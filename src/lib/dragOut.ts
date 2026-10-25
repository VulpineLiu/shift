/**
 * Payload for dragging a library row out of Shift.
 *
 * A row is dragged as a file URL rather than as text: WebKit promotes a `file://`
 * entry in `text/uri-list` into a real file drop, so Finder, mail composers and chat
 * apps receive the file instead of a path string. Plain text still carries the path for
 * editors, and Shift's own drop zones read the internal type so a row can be dragged
 * back onto the ring.
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

/** The three flavours a receiving application can pick up, keyed by MIME type. */
export function dragOutPayload(path: string): Record<string, string> {
  const uri = fileUrl(path);
  return {
    "text/uri-list": uri,
    "text/plain": path,
    [SHARE_PATH_TYPE]: JSON.stringify([path]),
  };
}

export const SHARE_PATHS_TYPE = SHARE_PATH_TYPE;
