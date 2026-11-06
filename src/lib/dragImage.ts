import { extensionOf } from "./utils.ts";

/**
 * The picture the operating system carries while a file is dragged.
 *
 * Left to itself the engine photographs the element under the pointer, and a Recent row
 * is a thumbnail, a name, an age, a folder path and five hover buttons - a piece of the
 * list rather than a file, which reads as though the whole list were being dragged. This
 * builds the ghost instead: one file, named, the same figure the ring draws for the file
 * it is about to convert.
 */
export function fileDragImage(
  dataTransfer: Pick<DataTransfer, "setDragImage">,
  name: string,
  thumbnail: HTMLImageElement | null,
): void {
  const ghost = document.createElement("div");
  ghost.className = "file-drag-ghost";
  const art = document.createElement("div");
  art.className = `file-drag-art ${thumbnail ? "is-image" : ""}`;
  if (thumbnail) {
    // Cloned rather than re-pointed at the same URL: the list has this picture decoded
    // already, and a new `img` may not have finished loading by the time the engine
    // photographs the ghost, which would ship a blank card.
    art.append(thumbnail.cloneNode());
  } else {
    // Nothing to picture, so name the kind of file the way the list does.
    art.textContent = extensionOf(name) || "file";
  }
  const label = document.createElement("span");
  label.textContent = name;
  ghost.append(art, label);
  document.body.append(ghost);
  // Anchored at the pointer, so the file itself is what sits under the cursor.
  dataTransfer.setDragImage(ghost, 60, 60);
  // The engine photographs the element as part of `dragstart`, and the two engines do
  // it on different ticks, so it has to outlive a frame or two. It sits off screen and
  // out of the page's flow the whole time, so lingering costs nothing visible.
  window.setTimeout(() => ghost.remove(), 250);
}
