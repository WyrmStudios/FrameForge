// [console-login feature] — delete this file to remove the feature
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ConsoleLoginSuccessPayload } from "./types/tauri";

interface Props {
  onLogin: (accountId: string, nonce: string) => void;
}

export default function ConsoleLogin({ onLogin }: Props) {
  const [status, setStatus] = useState<"idle" | "waiting" | "done" | "error">("idle");
  const [msg, setMsg]       = useState<string | null>(null);

  useEffect(() => {
    const ul = listen<ConsoleLoginSuccessPayload>("console-login-success", e => {
      setStatus("done");
      setMsg("Session captured — inventory access active.");
      onLogin(e.payload.accountId, e.payload.nonce);
    });
    return () => { ul.then(fn => fn()); };
  }, [onLogin]);

  const open = async () => {
    setStatus("waiting");
    setMsg(null);
    try {
      await invoke("open_console_login");
    } catch (e) {
      setStatus("error");
      setMsg(String(e));
    }
  };

  const cancel = () => {
    setStatus("idle");
    setMsg(null);
  };

  return (
    <div className="settings-section" style={{ borderTop: "1px solid rgba(255,255,255,.06)", marginTop: 12, paddingTop: 12 }}>
      <div className="settings-section-title" style={{ display: "flex", alignItems: "center", gap: 8 }}>
        Console / Web Login
        <span style={{
          fontSize: 9, fontWeight: 700, letterSpacing: ".06em",
          background: "rgba(255,180,0,.15)", color: "#ffb400",
          border: "1px solid rgba(255,180,0,.3)", borderRadius: 4,
          padding: "1px 5px",
        }}>EXPERIMENTAL</span>
      </div>

      <div style={{ fontSize: 11, color: "var(--muted)", marginBottom: 10, lineHeight: 1.6 }}>
        Opens warframe.com in a secure browser window. Log in with any method —
        PlayStation, Xbox, Nintendo, or email/password. FrameForge intercepts
        the session automatically and closes the window.
      </div>

      {status !== "waiting" && (
        <button
          className="btn-secondary"
          onClick={open}
          style={status === "done" ? { opacity: 0.5 } : undefined}
        >
          {status === "done" ? "Re-open Login" : "Open Warframe Login"}
        </button>
      )}

      {status === "waiting" && (
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <span style={{ fontSize: 12, color: "var(--muted)" }}>Waiting for login…</span>
          <button className="btn-secondary" style={{ padding: "2px 10px", fontSize: 11 }} onClick={cancel}>
            Cancel
          </button>
        </div>
      )}

      {msg && (
        <div className="settings-msg" style={{ marginTop: 6, color: status === "error" ? "var(--red)" : "var(--green)" }}>
          {msg}
        </div>
      )}
    </div>
  );
}
