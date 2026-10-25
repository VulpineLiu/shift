/**
 * The wedge catalogue for the radial rings, plus the ordering rules behind the
 * "custom radial order" setting.
 *
 * The rings used to hard-code their wedge lists inline in RadialMenu, which made a
 * user-chosen order impossible to express. Now every ring is described here as a
 * list of wedge ids for a file kind, and the menu only ever renders
 * `orderWedgeIds(defaultWedgeIds(...), savedOrder)` — so the default order is
 * whatever this module says, and a saved order can only rearrange those ids.
 * Ids the catalogue no longer offers are dropped, and new ids slide in at their
 * default position, so a saved order never hides or duplicates a wedge.
 */
import { conversionFormats } from "./conversionFormats.ts";
import type { FileInfo } from "../types";

export type RadialRing = "actions" | "formats" | "tools";
export type FileKind = FileInfo["kind"];

/** Per-kind and per-ring saved orders, keyed by `wedgeOrderKey`. */
export type WedgeOrders = Record<string, string[]>;

/** Kinds that get their own preset in Settings, in display order. */
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

/**
 * A typical source extension per kind, so the reorder screen can show the same wedge
 * list the ring would build for that kind. The convert wedge only exists when the kind
 * has targets, and that depends on the source extension rather than the kind alone.
 */
const PRESET_SOURCE: Record<FileKind, string> = {
  image: "png",
  video: "mp4",
  audio: "wav",
  pdf: "pdf",
  document: "docx",
  presentation: "pptx",
  spreadsheet: "xlsx",
  other: "zip",
};

/**
 * The wedge list the reorder screen shows for a preset: what the ring offers for a
 * typical source of that kind, multi-select included. Exported so the ordering rules
 * stay testable without mounting the settings panel.
 */
export function presetWedgeIds(kind: FileKind, ring: RadialRing): string[] {
  const formats = ring === "formats" ? [] : conversionFormats(kind, PRESET_SOURCE[kind]);
  return defaultWedgeIds(ring, kind, formats, true);
}

/** Move one entry of an order list up or down; out-of-range moves are no-ops. */
export function moveWedge(ids: string[], index: number, delta: number): string[] {
  const target = index + delta;
  if (index < 0 || index >= ids.length || target < 0 || target >= ids.length) return [...ids];
  const next = [...ids];
  [next[index], next[target]] = [next[target], next[index]];
  return next;
}

/**
 * Wedge ids whose label key differs from the id itself, so the reorder list can
 * reuse the wording the ring already uses.
 */
const LABEL_KEYS: Record<string, string> = {
  grayscale: "blackWhite",
  resizeHalf: "halfSize",
  pdfOcr: "ocr",
  text: "extractText",
  send: "sendShort",
  // The wedge removes metadata and writes a cleaned copy; it never shows what a file
  // carries. The id stays `metadata` so orders saved by earlier builds still resolve.
  metadata: "stripMetadata",
};

export function wedgeLabelKey(id: string): string {
  return LABEL_KEYS[id] ?? id;
}

/** Whether any ring of this kind carries a custom order, for the preset markers. */
export function hasCustomOrder(orders: WedgeOrders, kind: FileKind): boolean {
  return Object.keys(orders).some(
    (key) => key.startsWith(`${kind}/`) && (orders[key]?.length ?? 0) > 0,
  );
}

/** Whether a draft differs from what is stored, i.e. whether saving does anything. */
export function isOrderDirty(
  draft: string[],
  catalogue: string[],
  saved?: string[],
): boolean {
  const baseline = orderWedgeIds(catalogue, saved);
  return draft.length !== baseline.length || draft.some((id, index) => id !== baseline[index]);
}
