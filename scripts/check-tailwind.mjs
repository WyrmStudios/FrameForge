import { spawn } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const ROOT = process.cwd();
const ROOT_URI = pathToFileURL(`${ROOT}${path.sep}`).href;
const SERVER_DIRECTORY = path.join(ROOT, "node_modules", "@tailwindcss", "language-server", "bin");
const SOURCE_EXTENSION = /\.(?:css|html|js|jsx|ts|tsx)$/;
const EXCLUDED_DIRECTORIES = new Set([".git", "dist", "node_modules", "target"]);
const DIAGNOSTIC_WAIT_MS = 12_000;
const jsonOutput = process.argv.includes("--json");

async function collectFiles(directory, files = []) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    if (EXCLUDED_DIRECTORIES.has(entry.name)) continue;
    const fullPath = path.join(directory, entry.name);
    if (entry.isDirectory()) await collectFiles(fullPath, files);
    else if (SOURCE_EXTENSION.test(entry.name)) files.push(fullPath);
  }
  return files;
}

function languageId(file) {
  const extension = path.extname(file).slice(1);
  return {
    js: "javascript",
    jsx: "javascriptreact",
    ts: "typescript",
    tsx: "typescriptreact",
  }[extension] ?? extension;
}

function delay(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function checkWithServer(serverName, files) {
  const server = spawn(
    process.execPath,
    [path.join(SERVER_DIRECTORY, serverName), "--stdio"],
    { cwd: ROOT, stdio: ["pipe", "pipe", "pipe"] },
  );

  let buffer = Buffer.alloc(0);
  let nextId = 1;
  const pending = new Map();
  const diagnostics = new Map();
  const serverErrors = [];
  const serverExited = new Promise((resolve) => server.once("exit", resolve));

  function send(message) {
    const body = Buffer.from(JSON.stringify(message));
    server.stdin.write(`Content-Length: ${body.length}\r\n\r\n`);
    server.stdin.write(body);
  }

  function respond(id, result = null) {
    send({ jsonrpc: "2.0", id, result });
  }

  function request(method, params) {
    const id = nextId++;
    send({ jsonrpc: "2.0", id, method, params });
    return new Promise((resolve, reject) => pending.set(id, { resolve, reject }));
  }

  function notify(method, params) {
    send({ jsonrpc: "2.0", method, params });
  }

  function handleMessage(message) {
    if (message.id !== undefined && !message.method) {
      const callback = pending.get(message.id);
      if (!callback) return;
      pending.delete(message.id);
      if (message.error) callback.reject(new Error(message.error.message));
      else callback.resolve(message.result);
      return;
    }

    if (message.method === "textDocument/publishDiagnostics") {
      diagnostics.set(message.params.uri, message.params.diagnostics);
      return;
    }

    if (message.id === undefined) return;
    if (message.method === "workspace/configuration") {
      const config = {
        editor: { tabSize: 2 },
        tailwindCSS: {
          validate: true,
          lint: {
            cssConflict: "warning",
            invalidApply: "error",
            invalidConfigPath: "error",
            invalidScreen: "error",
            invalidTailwindDirective: "error",
            invalidVariant: "error",
            recommendedVariantOrder: "warning",
            suggestCanonicalClasses: "warning",
          },
        },
      };
      respond(message.id, message.params.items.map(({ section }) => config[section] ?? null));
    } else if (message.method === "workspace/workspaceFolders") {
      respond(message.id, [{ uri: ROOT_URI, name: path.basename(ROOT) }]);
    } else {
      respond(message.id);
    }
  }

  server.stdout.on("data", (chunk) => {
    buffer = Buffer.concat([buffer, chunk]);
    while (true) {
      const headerEnd = buffer.indexOf("\r\n\r\n");
      if (headerEnd < 0) return;
      const header = buffer.subarray(0, headerEnd).toString();
      const match = /Content-Length:\s*(\d+)/i.exec(header);
      if (!match) throw new Error(`Invalid LSP header from ${serverName}`);
      const length = Number(match[1]);
      const bodyStart = headerEnd + 4;
      if (buffer.length < bodyStart + length) return;
      const body = buffer.subarray(bodyStart, bodyStart + length).toString();
      buffer = buffer.subarray(bodyStart + length);
      handleMessage(JSON.parse(body));
    }
  });
  server.stderr.on("data", (chunk) => serverErrors.push(chunk.toString()));
  server.stdin.on("error", (error) => {
    if (error.code !== "EPIPE") serverErrors.push(error.stack ?? error.message);
  });

  try {
    await request("initialize", {
      processId: process.pid,
      rootPath: ROOT,
      rootUri: ROOT_URI,
      workspaceFolders: [{ uri: ROOT_URI, name: path.basename(ROOT) }],
      capabilities: {
        workspace: { configuration: true, workspaceFolders: true },
        textDocument: { publishDiagnostics: { relatedInformation: true } },
      },
    });
    notify("initialized", {});
    await delay(3_000);

    for (const file of files) {
      notify("textDocument/didOpen", {
        textDocument: {
          uri: pathToFileURL(file).href,
          languageId: languageId(file),
          version: 1,
          text: await readFile(file, "utf8"),
        },
      });
    }

    await delay(DIAGNOSTIC_WAIT_MS);
    const output = [];
    for (const [uri, items] of diagnostics) {
      for (const item of items) {
        output.push({
          server: serverName,
          file: path.relative(ROOT, fileURLToPath(uri)),
          line: item.range.start.line + 1,
          column: item.range.start.character + 1,
          severity: item.severity,
          code: item.code,
          message: item.message,
        });
      }
    }
    output.sort((a, b) => a.file.localeCompare(b.file) || a.line - b.line || a.column - b.column);

    await request("shutdown", null);
    notify("exit");
    await Promise.race([serverExited, delay(1_000)]);
    return output;
  } catch (error) {
    const details = serverErrors.join("").trim();
    throw new Error(`${serverName} failed: ${error.message}${details ? `\n${details}` : ""}`);
  } finally {
    if (server.exitCode === null && !server.killed) server.kill();
  }
}

const files = (await collectFiles(ROOT)).sort();
const results = [];
for (const serverName of ["tailwindcss-language-server", "css-language-server"]) {
  results.push(...await checkWithServer(serverName, files));
}

if (jsonOutput) {
  console.log(JSON.stringify({ filesChecked: files.length, diagnostics: results }, null, 2));
} else if (results.length === 0) {
  console.log(`Tailwind diagnostics passed (${files.length} files checked).`);
} else {
  const severity = { 1: "error", 2: "warning", 3: "info", 4: "hint" };
  for (const item of results) {
    const code = item.code ? ` ${item.code}` : "";
    console.error(`${item.file}:${item.line}:${item.column} [${severity[item.severity] ?? "diagnostic"}${code}] ${item.message}`);
  }
  console.error(`${results.length} Tailwind diagnostic${results.length === 1 ? "" : "s"} found.`);
  process.exitCode = 1;
}
