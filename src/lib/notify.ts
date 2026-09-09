import { isPermissionGranted, requestPermission, sendNotification } from "@tauri-apps/plugin-notification";

// Only an in-flight prompt is memoised, so concurrent callers share one dialog
// without the answer being cached for the run: permission can change in OS
// settings at any point, and a remembered "denied" would outlive the change.
let pending: Promise<boolean> | null = null;

// Call from a user gesture, never from a timer: an OS prompt raised by a
// background poll arrives minutes after anything the user did, with nothing
// on screen to explain it.
export function ensurePermission(): Promise<boolean> {
  pending ??= (async () => {
    try {
      return (await isPermissionGranted()) || (await requestPermission()) === "granted";
    } catch (e) {
      console.error("notification permission request failed", e);
      return false;
    } finally {
      pending = null;
    }
  })();
  return pending;
}

// Never prompts and never throws: a missing notification daemon or a denied
// permission must not break the caller's poll loop.
export async function notify(title: string, body: string): Promise<void> {
  try {
    if (!(await isPermissionGranted())) return;
    sendNotification({ title, body });
  } catch (e) {
    console.error("notification failed", e);
  }
}
