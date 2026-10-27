/**
 * The wedge catalogue for the radial rings, plus the ordering rules for saved
 * `settings.json` entries. The menu renders `orderWedgeIds(defaultWedgeIds(...),
 * savedOrder)`, so a saved order can only rearrange ids the catalogue still
 * offers; removed ids disappear and new ids keep their default position.
 */
import type { FileInfo } from "../types";

export type RadialRing = "actions" | "formats" | "tools";
export type FileKind = FileInfo["kind"];

/** Per-kind and per-ring saved orders, keyed by `wedgeOrderKey`. */
export type WedgeOrders = Record<string, string[]>;

/** Every file kind that can appear in a saved order. */
export const ORDERABLE_KINDS: FileKind[] = [
  "image",
  "video",
  "audio",
  "pdf",
  "document",
  "presentation",
  "spreadsheet",
  "other",
];

const IMAGE_ACTIONS = [
  "compress",
  "metadata",
  "edit",
  "annotate",
  "addBackground",
  "crop",
  "redact",
  "ocr",
  "convert",
];
const IMAGE_TOOLS = [
  "rotate",
  "flip",
  "grayscale",
  "resizeHalf",
  "compress",
  "metadata",
  "crop",
  "cutout",
];
const MEDIA_ACTIONS = ["convert", "compress", "edit", "reveal"];
const VIDEO_TOOLS = ["compress", "hd720", "mute", "extractAudio", "frame", "reveal"];
const AUDIO_TOOLS = ["compress", "mono", "reveal"];
const PDF_ACTIONS = [
  "convert",
  "compress",
  "pdfOcr",
  "text",
  "split",
  "rotate",
  "metadata",
  "reveal",
];
const PDF_TOOLS = ["compress", "metadata", "rotate", "split", "text", "reveal"];
const DOCUMENT_ACTIONS = ["convert", "edit", "open", "reveal"];
const DOCUMENT_TOOLS = ["plain", "reveal"];
const PLAIN_ACTIONS = ["open", "reveal"];
const PLAIN_TOOLS = ["open", "reveal"];

/** Kinds whose rings gain a Merge wedge when 2+ files are selected. */
const MERGEABLE: FileKind[] = ["image", "pdf"];

/**
 * Default wedge order for one ring of one file kind.
 *
 * `formats` is the format catalogue for the source (see conversionFormats); it is a
 * parameter because it depends on the source extension, which this module does not
 * know. `multi` mirrors the merge-wedge rule in the menu.
 */
export function defaultWedgeIds(
  ring: RadialRing,
  kind: FileKind,
  formats: string[] = [],
  multi = false,
): string[] {
  switch (ring) {
    case "formats":
      return formats.map((format) => format.toLowerCase());
    case "tools":
      return withMerge(toolsFor(kind), kind, multi);
    case "actions":
      return withMerge(actionsFor(kind, formats.length), kind, multi);
  }
}

function toolsFor(kind: FileKind): string[] {
  switch (kind) {
    case "image":
      return IMAGE_TOOLS;
    case "video":
      return VIDEO_TOOLS;
    case "audio":
      return AUDIO_TOOLS;
    case "pdf":
      return PDF_TOOLS;
    case "document":
    case "presentation":
    case "spreadsheet":
      return DOCUMENT_TOOLS;
    default:
      return PLAIN_TOOLS;
  }
}

function actionsFor(kind: FileKind, formatCount: number): string[] {
  switch (kind) {
    case "image":
      return IMAGE_ACTIONS;
    case "video":
    case "audio":
      return MEDIA_ACTIONS;
    case "pdf":
      return PDF_ACTIONS;
    case "document":
    case "presentation":
    case "spreadsheet":
      // A source that cannot convert has no convert wedge to order.
      return formatCount > 0 ? DOCUMENT_ACTIONS : DOCUMENT_ACTIONS.slice(1);
    default:
      return PLAIN_ACTIONS;
  }
}

function withMerge(ids: string[], kind: FileKind, multi: boolean): string[] {
  return multi && MERGEABLE.includes(kind) ? [...ids, "merge"] : ids;
}

/** Send is appended by the menu after ordering, so it always sits last. */
export const ALWAYS_LAST = "send";

/**
 * Every action id any catalogue mentions, derived rather than listed so a new wedge
 * cannot be added to one ring and forgotten here. The menu cross-checks this against
 * its icon table in development, which is what catches a wedge rendered with the
 * generic fallback icon.
 */
export const WEDGE_ACTION_IDS: string[] = Array.from(
  new Set(
    ORDERABLE_KINDS.flatMap((kind) => [
      ...defaultWedgeIds("actions", kind, ["PNG", "PDF"], true),
      ...defaultWedgeIds("tools", kind, [], true),
    ]),
  ),
).filter((id) => id !== ALWAYS_LAST);

/**
 * Apply a saved order to the wedges this ring actually offers. Unknown ids are
 * ignored, missing ones keep their default slot.
 */
export function orderWedgeIds(ids: string[], order?: string[]): string[] {
  if (!order || order.length === 0) return [...ids];
  const leading = order.filter((id) => ids.includes(id));
  const seen = new Set(leading);
  const rest = ids.filter((id) => !seen.has(id));
  return [...leading, ...rest];
}

export function wedgeOrderKey(kind: FileKind, ring: RadialRing): string {
  return `${kind}/${ring}`;
}

export function savedOrderFor(
  orders: WedgeOrders | undefined,
  kind: FileKind,
  ring: RadialRing,
): string[] | undefined {
  const value = orders?.[wedgeOrderKey(kind, ring)];
  return Array.isArray(value) && value.length > 0 ? value : undefined;
}
