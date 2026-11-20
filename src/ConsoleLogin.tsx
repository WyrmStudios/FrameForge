// [console-login feature] — delete this file to remove the feature
import { useState, useEffect } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { TAURI_EVENTS } from "./constants/tauri";
import { SecondaryButton } from "./shared/ui/ActionButton";
import type { ConsoleLoginSuccessPayload } from "./types/tauri";

interface Props {
  onLogin: (accountId: string, nonce: string) => void;
}

export default function ConsoleLogin({ onLogin }: Props) {
  const [status, setStatus] = useState<"idle" | "waiting" | "done" | "error">("idle");
  const [msg, setMsg]       = useState<string | null>(null);

  useEffect(() => {
    const ul = listen<ConsoleLoginSuccessPayload>(TAURI_EVENTS.CONSOLE_LOGIN_SUCCESS, e => {
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
    <div className="mt-3 border-t border-t-white/6 border-b border-b-border/60 px-5 pt-3 pb-3 last:border-b-0">
      <div className="mb-2.5 flex items-center gap-2 text-10 font-bold uppercase tracking-0.07 text-muted">
        Console / Web Login
        <span className="rounded-4 border border-experimental/30 bg-experimental/15 px-1.25 py-px text-9 font-bold tracking-0.06 text-experimental">EXPERIMENTAL</span>
      </div>

      <div className="mb-2.5 text-11 leading-1.6 text-muted">
        Opens warframe.com in a secure browser window. Log in with any method —
        PlayStation, Xbox, Nintendo, or email/password. FrameForge intercepts
        the session automatically and closes the window.
      </div>

      {status !== "waiting" && (
        <SecondaryButton
          onClick={open}
          className={status === "done" ? "opacity-50" : undefined}
        >
          {status === "done" ? "Re-open Login" : "Open Warframe Login"}
        </SecondaryButton>
      )}

      {status === "waiting" && (
        <div className="flex items-center gap-2.5">
          <span className="text-12 text-muted">Waiting for login…</span>
          <SecondaryButton onClick={cancel}>
            Cancel
          </SecondaryButton>
        </div>
      )}

      {msg && (
        <div className={`mt-2 text-11 ${status === "error" ? "text-red" : "text-green"}`}>
          {msg}
        </div>
      )}
    </div>
  );
}
