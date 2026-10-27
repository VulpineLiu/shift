/**
 * Payload for dragging a library row out of Shift.
 *
 * A row is dragged as a file URL rather than as text: WebKit promotes a `file://`
 * entry in `text/uri-list` into a real file drop, so Finder, mail composers and chat
 * apps receive the file instead of a path string. Plain text still carries the path
 * for editors.
 */
const SHARE_PATH_TYPE = "application/x-shift-share-paths";

/** Encode a path segment but leave a Windows drive colon readable. */
function encodeSegment(segment: string): string {
  return encodeURIComponent(segment).replace(/%3A/gi, ":");
}

/** `file://` URL for a local path, percent-encoded but keeping path separators. */
export function fileUrl(path: string): string {
  const unc = path.startsWith("\\\\");
  const windows = /^[a-zA-Z]:[\\/]/.test(path) || unc;
  const segments = path.split(windows ? /[\\/]+/ : /\/+/).filter(Boolean).map(encodeSegment);
  return `file:///${segments.join("/")}`;
}

/** The flavours a receiving application can pick up, keyed by MIME type. */
export function dragOutPayload(path: string): Record<string, string> {
  const uri = fileUrl(path);
  return {
    "text/uri-list": uri,
    "text/plain": path,
  };
}

/** Read the internal path list from an in-app drag payload. */
export function parseSharePaths(raw: string): string[] | null {
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as unknown;
    if (Array.isArray(parsed) && parsed.every((path) => typeof path === "string")) {
      return parsed;
    }
  } catch {
    // Malformed payloads fall through so callers can use their own default.
  }
  return null;
}

export const SHARE_PATHS_TYPE = SHARE_PATH_TYPE;
