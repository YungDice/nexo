/**
 * The native seam: file pickers, save dialogs, clipboard, and the "not built
 * yet" notice, all in one place so components never touch a Tauri plugin
 * directly.
 *
 * Every call is wrapped the same way `windowAction` is (`useWindow.ts`):
 * `vite dev` in a browser has no Tauri runtime, so a missing plugin degrades
 * to a no-op instead of throwing.
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";

import { requestDialog } from "./dialogs";

export interface PickedFile {
  path: string;
  name: string;
  /** A `asset://` URL this WebView can render directly — images, mostly. */
  url: string;
}

function fileName(path: string): string {
  return path.split(/[\\/]/).pop() ?? path;
}

/** Opens the native Open File dialog (Explorer) and returns what was picked. */
export async function pickFile(options?: {
  title?: string;
  images?: boolean;
}): Promise<PickedFile | null> {
  try {
    const { open } = await import("@tauri-apps/plugin-dialog");
    const path = await open({
      multiple: false,
      directory: false,
      title: options?.title ?? "Choose a file",
      ...(options?.images
        ? { filters: [{ name: "Images", extensions: ["png", "jpg", "jpeg", "gif", "webp"] }] }
        : {}),
    });
    if (!path || Array.isArray(path)) return null;
    return { path, name: fileName(path), url: convertFileSrc(path) };
  } catch {
    return null;
  }
}

/**
 * Opens the native Save As dialog and returns the chosen path.
 *
 * Only the path: writing is Rust's job, because the bytes have to be
 * downloaded and decrypted first and neither belongs in the WebView. The
 * suggested name comes from the sender and has already been sanitised on the
 * Rust side — but the user picks the real destination, so a hostile name
 * cannot decide where a file lands.
 */
export async function pickSavePath(suggestedName: string): Promise<string | null> {
  try {
    const { save } = await import("@tauri-apps/plugin-dialog");
    const path = await save({ title: "Save attachment", defaultPath: suggestedName });
    return path ?? null;
  } catch {
    return null;
  }
}

/** Says something inside the app, as a toast that leaves on its own. */
export async function notify(title: string, body: string): Promise<void> {
  await requestDialog("info", title, body);
}

/** Asks inside the app and waits for the answer. */
export async function confirm(title: string, body: string): Promise<boolean> {
  return requestDialog("confirm", title, body);
}

/** Opens a URL in the system's default browser, never inside the WebView. */
export async function openUrl(url: string): Promise<void> {
  try {
    const { openUrl: open } = await import("@tauri-apps/plugin-opener");
    await open(url);
  } catch {
    window.open(url, "_blank", "noopener,noreferrer");
  }
}

/** How much a Windows toast is allowed to say (§8). Applied in Rust. */
export type NotificationDetail = "full" | "sender" | "none";

/**
 * Shows a Windows toast for an incoming message.
 *
 * The WebView asks; Rust decides what the toast actually says, by applying
 * `detail` in `toast_text` before anything reaches the OS. A toast is drawn on
 * the lock screen and over screen shares, so "the notification respects the
 * privacy setting" has to be true of the process that builds the string.
 */
export async function toastMessage(
  sender: string,
  body: string,
  detail: NotificationDetail,
): Promise<void> {
  try {
    await invoke("notify_message", { sender, body, detail });
  } catch {
    // A browser preview has no toasts, and a toast that could not be shown is
    // not worth interrupting a sync over.
  }
}

/** Updates the tray tooltip's unread count (§8). */
export async function setTrayUnread(unread: number): Promise<void> {
  try {
    await invoke("set_unread", { unread });
  } catch {
    // No tray in a browser preview.
  }
}

/**
 * Locks the app: Rust closes the encrypted store and drops the MLS state.
 *
 * Infallible by design — see the `lock` command. If this could fail and the
 * UI treated that as "stay unlocked", the failure mode would be an app that
 * looks locked and is not.
 */
export async function lockCore(): Promise<void> {
  try {
    await invoke("lock");
  } catch {
    // Nothing to lock in a browser preview.
  }
}

/** Pushes the close-to-tray preference to Rust, where the close handler lives. */
export async function setCloseToTray(enabled: boolean): Promise<void> {
  try {
    await invoke("set_close_to_tray", { enabled });
  } catch {
    // No window chrome to configure in a browser preview.
  }
}

export type BackdropKind = "off" | "acrylic" | "mica" | "tabbed" | "blur";

/** What came back from asking Windows for a backdrop. */
export interface BackdropReport {
  requested: BackdropKind;
  /**
   * The call went through and was not refused.
   *
   * Deliberately **not** a promise that the desktop is now visible through the
   * window. From Windows 11 build 22523 on, the API that sets the backdrop
   * does not report whether it took, so this is the most that can honestly be
   * claimed — and why Settings shows it next to a chooser rather than the app
   * deciding on its own.
   */
  applied: boolean;
  /** Plain words for the Settings panel. Empty when there is nothing to add. */
  note: string;
}

/**
 * Asks Windows for a blurred backdrop behind the window.
 *
 * The one thing CSS cannot do. `backdrop-filter` blurs what is behind an
 * element *in this document*; the desktop is not in this document, so the
 * wallpaper and the windows underneath can only be reached by the desktop
 * window manager.
 */
export async function setWindowBackdrop(kind: BackdropKind): Promise<BackdropReport> {
  try {
    return await invoke<BackdropReport>("set_window_backdrop", { kind });
  } catch {
    // No window to composite behind in a browser preview.
    return { requested: kind, applied: false, note: "No desktop window here." };
  }
}

/**
 * Whether the app starts with Windows, read from the registry.
 *
 * `null` when there is no runtime to ask — the Settings toggle shows itself
 * as unavailable rather than claiming a state it cannot know.
 */
export async function getAutostart(): Promise<boolean | null> {
  try {
    return await invoke<boolean>("get_autostart");
  } catch {
    return null;
  }
}

/** Turns start-with-Windows on or off. Resolves false when it could not. */
export async function setAutostart(enabled: boolean): Promise<boolean> {
  try {
    await invoke("set_autostart", { enabled });
    return true;
  } catch {
    return false;
  }
}

/** A link preview, fetched by this machine (§4.5). */
export interface LinkPreviewData {
  url: string;
  title: string;
  description: string;
  source: string;
}

/**
 * Fetches a preview for one URL.
 *
 * Only call this when the `linkPreviews` preference is on: fetching a link
 * reveals this machine's IP and rough activity to whoever controls it, which
 * is why the setting exists and why it is off by default. Rust enforces the
 * rest — https only, no private addresses, no redirects, byte and time
 * ceilings — regardless of who asked.
 *
 * Resolves `null` for anything that could not be previewed. A link with no
 * preview stays a link, which is the same thing the setting-off path renders.
 */
export async function previewLink(url: string): Promise<LinkPreviewData | null> {
  try {
    return await invoke<LinkPreviewData>("preview_link", { url });
  } catch {
    return null;
  }
}

/** What Nexo is keeping on this machine (§6.4). */
export interface StorageInfo {
  storePath: string;
  /** The encrypted database and its WAL sidecars. The only copy of your messages. */
  storeBytes: number;
  /** Downloaded media the WebView is holding. Re-fetchable, safe to clear. */
  cacheBytes: number;
}

/**
 * Measures the local store and the media cache.
 *
 * `null` when there is no runtime to ask, so the panel can say "unavailable"
 * rather than print an invented number.
 */
export async function storageInfo(): Promise<StorageInfo | null> {
  try {
    const raw = await invoke<{
      store_path: string;
      store_bytes: number;
      cache_bytes: number;
    }>("storage_info");
    return {
      storePath: raw.store_path,
      storeBytes: raw.store_bytes,
      cacheBytes: raw.cache_bytes,
    };
  } catch {
    return null;
  }
}

/** Clears downloaded media. The encrypted store is untouched. */
export async function clearMediaCache(): Promise<boolean> {
  try {
    await invoke("clear_media_cache");
    return true;
  } catch {
    return false;
  }
}

/** What an update check found. */
export interface UpdateInfo {
  version: string;
}

/**
 * Asks the update server for a newer build. Resolves `null` when this build is
 * current. Throws with a human-readable message when the check itself failed —
 * including in a dev build, which has no update key configured, and says so.
 */
export async function checkUpdate(): Promise<UpdateInfo | null> {
  return await invoke<UpdateInfo | null>("check_update");
}

/** Downloads and installs a waiting update, then restarts the app. */
export async function installUpdate(): Promise<void> {
  await invoke("install_update");
}

/**
 * What is on the clipboard, as text, or `null` if it cannot be read.
 *
 * # Why this exists at all
 *
 * The capability set is deliberately small and the clipboard was write-only
 * (`capabilities/default.json` said so in as many words). Reading it back is
 * a real widening, and it is here for exactly one reason: the app draws its
 * own right-click menu in text fields now, and a text field's menu without
 * Paste is not a text field's menu.
 *
 * # What it costs, stated plainly
 *
 * Clipboard text already reaches the WebView every time somebody presses
 * Ctrl+V — the browser inserts it into the DOM, where the page can read it.
 * What changes is who starts it: code running in the WebView can now ask
 * without being asked. In an app whose WebView already sees decrypted
 * messages that is a small step, but it is a step, and `docs/THREAT-MODEL.md`
 * records it rather than leaving it in a JSON file nobody reads.
 */
export async function pasteText(): Promise<string | null> {
  try {
    const { readText } = await import("@tauri-apps/plugin-clipboard-manager");
    return await readText();
  } catch {
    try {
      return await navigator.clipboard.readText();
    } catch {
      return null;
    }
  }
}

export async function copyText(text: string): Promise<boolean> {
  try {
    const { writeText } = await import("@tauri-apps/plugin-clipboard-manager");
    await writeText(text);
    return true;
  } catch {
    try {
      await navigator.clipboard.writeText(text);
      return true;
    } catch {
      return false;
    }
  }
}
