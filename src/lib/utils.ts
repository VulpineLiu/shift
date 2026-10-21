import { isTauri } from "@tauri-apps/api/core";
import { emit } from "@tauri-apps/api/event";

export function basename(path: string): string {
  return path.split(/[\\/]/).pop() || path;
}

export const IS_MAC = /Mac/i.test(navigator.platform) || /Mac OS X/i.test(navigator.userAgent);
export const OPTION_LABEL = IS_MAC ? "⌥" : "Alt";
export const MODIFIER_NAME = IS_MAC ? "Option" : "Ctrl";

export function dirname(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const index = normalized.lastIndexOf("/");
  return index > 0 ? normalized.slice(0, index) : normalized;
}

export function stripExtension(name: string): string {
  const index = name.lastIndexOf(".");
  return index > 0 ? name.slice(0, index) : name;
}

export function extensionOf(path: string): string {
  const name = basename(path);
  const index = name.lastIndexOf(".");
  return index > 0 ? name.slice(index + 1).toLowerCase() : "";
}

export function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  const units = ["B", "KB", "MB", "GB", "TB"];
  const index = Math.min(Math.floor(Math.log(bytes) / Math.log(1024)), units.length - 1);
  const value = bytes / 1024 ** index;
  return `${value >= 10 || index === 0 ? value.toFixed(0) : value.toFixed(1)} ${units[index]}`;
}

export function formatDuration(milliseconds: number): string {
  const totalSeconds = Math.max(0, Math.floor(milliseconds / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  const parts = hours > 0 ? [hours, minutes, seconds] : [minutes, seconds];
  return parts.map((part) => String(part).padStart(2, "0")).join(":");
}

/** Short relative label for the recent list, e.g. "just now", "5m", "3h", "2d". */
export function timeAgo(
  timestamp: number,
  translate: (key: string, vars?: Record<string, string | number>) => string,
): string {
  const minutes = Math.floor((Date.now() - timestamp) / 60_000);
  if (minutes < 1) return translate("agoNow");
  if (minutes < 60) return translate("agoMinutes", { count: minutes });
  const hours = Math.floor(minutes / 60);
  if (hours < 24) return translate("agoHours", { count: hours });
  return translate("agoDays", { count: Math.floor(hours / 24) });
}

export function clamp(value: number, min: number, max: number): number {
  return Math.min(max, Math.max(min, value));
}

export function normalizeSelection(
  startX: number,
  startY: number,
  endX: number,
  endY: number,
  viewportWidth: number,
  viewportHeight: number,
) {
  const left = Math.min(startX, endX);
  const top = Math.min(startY, endY);
  const width = Math.abs(endX - startX);
  const height = Math.abs(endY - startY);
  return {
    left,
    top,
    width,
    height,
    normalized: {
      x: left / viewportWidth,
      y: top / viewportHeight,
      width: width / viewportWidth,
      height: height / viewportHeight,
    },
  };
}

export interface RecentItem {
  path: string;
  kind: "image" | "video" | "text" | "file";
  at: number;
}

const RECENT_KEY = "shift.recent";
const RECENT_EVENT = "shift:recent";

export function getRecentItems(): RecentItem[] {
  try {
    const parsed = JSON.parse(localStorage.getItem(RECENT_KEY) || "[]");
    return Array.isArray(parsed) ? parsed.slice(0, 8) : [];
  } catch {
    return [];
  }
}

export function rememberRecent(item: RecentItem) {
  const current = getRecentItems().filter((entry) => entry.path !== item.path);
  localStorage.setItem(RECENT_KEY, JSON.stringify([item, ...current].slice(0, 8)));
  window.dispatchEvent(new Event(RECENT_EVENT));
  // Radial and editor windows record results too; tell the main window to re-read.
  if (isTauri()) void emit("app://recent-changed", item.path).catch(() => undefined);
}

export function setRecentItems(items: RecentItem[]) {
  localStorage.setItem(RECENT_KEY, JSON.stringify(items.slice(0, 8)));
  window.dispatchEvent(new Event(RECENT_EVENT));
}

/** Subscribe to recent-list changes from this window or any other Shift window. */
export function subscribeRecent(listener: () => void) {
  window.addEventListener(RECENT_EVENT, listener);
  window.addEventListener("storage", listener);
  return () => {
    window.removeEventListener(RECENT_EVENT, listener);
    window.removeEventListener("storage", listener);
  };
}

export function recentKind(path: string): RecentItem["kind"] {
  const extension = extensionOf(path);
  if (["png", "jpg", "jpeg", "webp", "tiff", "tif", "bmp", "gif", "heic", "avif"].includes(extension)) {
    return "image";
  }
  if (["mp4", "mov", "m4v", "avi", "mkv", "webm"].includes(extension)) {
    return "video";
  }
  if (["txt", "md", "csv", "tsv", "html", "htm"].includes(extension)) {
    return "text";
  }
  return "file";
}

