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

// Reports permission without asking for it, for callers that need to know at a
// moment when raising the dialog would be wrong.
export async function permissionGranted(): Promise<boolean> {
  try {
    return await isPermissionGranted();
  } catch (e) {
    console.error("notification permission check failed", e);
    return false;
  }
}

// Never prompts and never throws: a missing notification daemon or a denied
// permission must not break the caller's poll loop.
//
// The result says the notification was handed over, not that anyone saw it:
// sendNotification returns void, so the platform never reports delivery.
export async function notify(title: string, body: string): Promise<boolean> {
  try {
    if (!(await isPermissionGranted())) return false;
    sendNotification({ title, body });
    return true;
  } catch (e) {
    console.error("notification failed", e);
    return false;
  }
}
