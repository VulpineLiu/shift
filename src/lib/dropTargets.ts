/**
 * Geometry for "drop a file on a button to run just that action".
 *
 * Kept free of React and Tauri so the mapping rules can be reasoned about (and
 * checked) on their own.
 */

export type Rect = { left: number; top: number; right: number; bottom: number };

/** Slack around a button so a near miss still counts as aiming at it. */
const SLACK_X = 10;
const SLACK_Y = 12;

/**
 * Turn a wry drag position into CSS pixels inside the window.
 *
 * The two platforms disagree in scale, not in origin: AppKit hands over
 * `draggingLocation`, which is already webview-relative points, while the
 * Windows and Linux drag controllers map screen cursor coordinates to client
 * coordinates, but they remain in physical pixels. Only the second reading
 * needs the device pixel ratio removed.
 */
export function toWindowPoint(
  position: { x: number; y: number },
  platform: "mac" | "other",
  scale: number,
): { x: number; y: number } | null {
  if (platform === "mac") return position;
  const factor = scale || 1;
  return { x: position.x / factor, y: position.y / factor };
}

/** The button a point sits on, or null when the drop is outside every button. */
export function hitTestDropTarget<T extends string>(
  targets: Array<[T, Rect | null | undefined]>,
  point: { x: number; y: number },
): T | null {
  for (const [action, rect] of targets) {
    if (!rect) continue;
    if (
      point.x >= rect.left - SLACK_X &&
      point.x <= rect.right + SLACK_X &&
      point.y >= rect.top - SLACK_Y &&
      point.y <= rect.bottom + SLACK_Y
    ) {
      return action;
    }
  }
  return null;
}
