/**
 * Which of the three home-panel buttons can serve the files a drag is carrying.
 *
 * One rule for both platforms and both delivery paths: a button that can do something
 * with the drag is a candidate, a button that cannot is spent for the length of the
 * gesture and runs nothing when the file is released on it. It used to be that only
 * 文字识别 spent, and that a file the ring has no target for still opened a wheel.
 *
 * The rule is deliberately one-way: a button is spent only when the files are known
 * *and* known not to be servable. A drag that reports no files — which is what a
 * Windows drag can look like before its payload is readable — leaves every button a
 * candidate, because guessing from an empty list would spend buttons that do work.
 */
import { conversionFormats } from "./conversionFormats.ts";
import { extensionOf } from "./utils.ts";
import type { FileInfo } from "../types";

export type DropAction = "formats" | "tools" | "ocr";

type FileKind = FileInfo["kind"];

/** The same extension tables `src-tauri/src/files.rs::kind_of` classifies with. */
export const IMAGE_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "webp",
  "tiff",
  "tif",
  "bmp",
  "gif",
  "heic",
  "heif",
  "avif",
];
const VIDEO = ["mp4", "mov", "m4v", "avi", "mkv", "webm"];
const AUDIO = ["mp3", "wav", "m4a", "aiff", "aif", "caf", "flac", "ogg"];
const DOCUMENT = ["docx", "doc", "rtf", "odt", "txt", "md", "html", "htm", "pages"];
const PRESENTATION = ["pptx", "ppt", "key"];
const SPREADSHEET = ["xlsx", "xls", "xlsb", "ods", "csv", "tsv", "numbers"];

/** Classify by extension, so a decision exists before the file reaches a button. */
export function kindFromPath(path: string): FileKind {
  const extension = extensionOf(path);
  if (IMAGE_EXTENSIONS.includes(extension)) return "image";
  if (VIDEO.includes(extension)) return "video";
  if (AUDIO.includes(extension)) return "audio";
  if (extension === "pdf") return "pdf";
  if (DOCUMENT.includes(extension)) return "document";
  if (PRESENTATION.includes(extension)) return "presentation";
  if (SPREADSHEET.includes(extension)) return "spreadsheet";
  return "other";
}

/** Whether OCR can read every file in the drag. The button says OCR, so images only. */
export function ocrCanRead(paths: string[]): boolean {
  if (paths.length === 0) return false;
  return paths.every((path) => IMAGE_EXTENSIONS.includes(extensionOf(path)));
}

/**
 * Whether each file can become something else: the ring's own type-awareness moved
 * forward in time. A `.key` has no conversion target, so 文件转换 spends rather than
 * opening a wheel with nothing in it.
 */
export function convertible(paths: string[]): boolean {
  if (paths.length === 0) return false;
  return paths.every(
    (path) => conversionFormats(kindFromPath(path), extensionOf(path)).length > 0,
  );
}

/**
 * Whether one button can serve the drag. `null` is a drag whose files are not known
 * yet, which is not a refusal: every button stays a candidate.
 */
export function actionCanServe(action: DropAction, paths: string[] | null): boolean {
  if (!paths || paths.length === 0) return true;
  switch (action) {
    case "formats":
      return convertible(paths);
    case "ocr":
      return ocrCanRead(paths);
    case "tools":
      // The tool ring ends in Open, Reveal and Send, which serve any file that exists.
      return true;
  }
}

/**
 * The cursor the engine should show for a button under the pointer.
 *
 * A spent button must say "no" before release, not only by greying out: leaving
 * `dropEffect` at `copy` tells the operating system the drop is welcome, which is the
 * mismatch users see as "it looks processable, but releasing does nothing". No button
 * under the pointer is also no drop: the panel itself runs nothing.
 */
export function dropEffectFor(
  action: DropAction | null,
  paths: string[] | null,
): "copy" | "none" {
  return action && actionCanServe(action, paths) ? "copy" : "none";
}

/**
 * What the drag in flight carries, kept across the two channels that report it.
 *
 * A Recent row registers its own path at `dragstart`, and the native channel can
 * report the same gesture with no file names at all: a drag the page started carries a
 * URI list, not the file list the native event reads. An empty report therefore means
 * "nothing new", not "nothing carried" — writing `[]` over the row's path makes every
 * button a candidate again, which is the mismatch of a processable cursor that hands
 * back no action. Nothing known and nothing reported stays unknown, the one case where
 * a button must not be spent.
 */
export function dragPathsKnown(known: string[] | null, reported: string[]): string[] | null {
  return reported.length > 0 ? reported : known;
}

/** The extensions the image picker offers when OCR is started by hand. */
export const OCR_IMAGE_EXTENSIONS = IMAGE_EXTENSIONS;
