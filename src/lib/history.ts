import { isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import {
  announceRecentChange,
  getLocalRecentItems,
  type RecentItem,
  setLocalRecentItems,
} from "./utils";

/**
 * Frontend half of the history library. Rust owns the list and writes it the
 * moment a capture, conversion or received file lands, so a window never has to
 * report its own results; this module just reads the list, applies the handful of
 * edits the UI offers, and re-renders when Rust broadcasts a change. Outside
 * Tauri the same API reads and writes the browser store so a plain webview
 * preview still shows something.
 */
async function request<T>(command: string, args?: Record<string, unknown>): Promise<T> {
  return invoke<T>(command, args);
}

function fallback(list: RecentItem[]): RecentItem[] {
  setLocalRecentItems(list);
  return list;
}

export const history = {
  async list(): Promise<RecentItem[]> {
    if (!isTauri()) return getLocalRecentItems();
    return request<RecentItem[]>("history_list");
  },

  /** Record paths that just became available, e.g. an editor save. */
  async record(paths: string[]): Promise<RecentItem[]> {
    const usable = paths.filter(Boolean);
    if (usable.length === 0) return this.list();
    if (!isTauri()) {
      const fresh: RecentItem[] = usable.map((path) => ({
        path,
        kind: "file",
        at: Date.now(),
        pinned: false,
      }));
      const merged = [...fresh, ...getLocalRecentItems().filter((e) => !usable.includes(e.path))];
      return fallback(merged);
    }
    return request<RecentItem[]>("history_record", { paths: usable });
  },

  async setPinned(path: string, pinned: boolean): Promise<RecentItem[]> {
    if (!isTauri())
      return fallback(
        getLocalRecentItems().map((entry) => (entry.path === path ? { ...entry, pinned } : entry)),
      );
    return request<RecentItem[]>("history_set_pinned", { path, pinned });
  },

  async remove(path: string): Promise<RecentItem[]> {
    if (!isTauri())
      return fallback(getLocalRecentItems().filter((entry) => entry.path !== path));
    return request<RecentItem[]>("history_remove", { path });
  },

  async clear(): Promise<RecentItem[]> {
    if (!isTauri()) return fallback([]);
    return request<RecentItem[]>("history_clear");
  },

  /** Rust republishes the whole list on every change, including from other windows. */
  onChanged(handler: (items: RecentItem[]) => void) {
    if (!isTauri()) {
      // The browser store only changes in this window, so a local event is enough.
      const listener = () => handler(getLocalRecentItems());
      window.addEventListener("shift:recent", listener);
      return () => window.removeEventListener("shift:recent", listener);
    }
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void listen<RecentItem[]>("history://changed", (event) => {
      if (!disposed) handler(event.payload);
    }).then((stop) => {
      if (disposed) stop();
      else unlisten = stop;
    });
    return () => {
      disposed = true;
      unlisten?.();
    };
  },
};

/** Kept so a caller that only has a path can still announce a local change. */
export { announceRecentChange };
