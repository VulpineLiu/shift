import assert from "node:assert/strict";
import { test } from "node:test";

import {
  ORDERABLE_KINDS,
  WEDGE_ACTION_IDS,
  defaultWedgeIds,
  hasCustomOrder,
  isOrderDirty,
  moveWedge,
  orderWedgeIds,
  presetWedgeIds,
  savedOrderFor,
  wedgeLabelKey,
  wedgeOrderKey,
} from "./radialOrder.ts";

const IMAGE_FORMATS = ["PNG", "JPG", "WEBP", "HEIC", "TIFF", "BMP", "GIF", "PDF"];

test("image action ring keeps its default catalogue", () => {
  assert.deepEqual(defaultWedgeIds("actions", "image", IMAGE_FORMATS), [
    "compress",
    "metadata",
    "edit",
    "annotate",
    "addBackground",
    "crop",
    "redact",
    "ocr",
    "convert",
  ]);
});

test("image tool ring keeps its default catalogue", () => {
  assert.deepEqual(defaultWedgeIds("tools", "image"), [
    "rotate",
    "flip",
    "grayscale",
    "resizeHalf",
    "compress",
    "metadata",
    "crop",
    "cutout",
  ]);
});

test("merge joins only image and pdf rings once files are selected", () => {
  assert.deepEqual(defaultWedgeIds("tools", "image", [], true).slice(-1), ["merge"]);
  assert.deepEqual(defaultWedgeIds("tools", "pdf", [], true).slice(-1), ["merge"]);
  assert.equal(defaultWedgeIds("tools", "video", [], true).includes("merge"), false);
  assert.equal(defaultWedgeIds("tools", "image", [], false).includes("merge"), false);
  assert.equal(defaultWedgeIds("actions", "image", IMAGE_FORMATS, false).includes("merge"), false);
});

test("a document that cannot convert has no convert wedge", () => {
  assert.deepEqual(defaultWedgeIds("actions", "document", []), [
    "edit",
    "open",
    "reveal",
  ]);
  assert.deepEqual(defaultWedgeIds("actions", "document", ["PDF"]), [
    "convert",
    "edit",
    "open",
    "reveal",
  ]);
});

test("the format ring is the catalogue lowercased", () => {
  assert.deepEqual(defaultWedgeIds("formats", "image", IMAGE_FORMATS), [
    "png",
    "jpg",
    "webp",
    "heic",
    "tiff",
    "bmp",
    "gif",
    "pdf",
  ]);
});

test("a saved order leads and unknown ids are dropped", () => {
  const ids = ["compress", "ocr", "convert"];
  assert.deepEqual(orderWedgeIds(ids, ["convert", "stale", "ocr"]), ["convert", "ocr", "compress"]);
});

test("an order missing a wedge leaves it in default relative order", () => {
  const ids = ["rotate", "flip", "crop", "newcomer"];
  assert.deepEqual(orderWedgeIds(ids, ["crop", "rotate"]), ["crop", "rotate", "flip", "newcomer"]);
});

test("no saved order means the default order", () => {
  const ids = ["a", "b"];
  assert.deepEqual(orderWedgeIds(ids), ids);
  assert.deepEqual(orderWedgeIds(ids, []), ids);
});

test("moving a wedge clamps at both ends", () => {
  const ids = ["a", "b", "c"];
  assert.deepEqual(moveWedge(ids, 0, -1), ids);
  assert.deepEqual(moveWedge(ids, 2, 1), ids);
  assert.deepEqual(moveWedge(ids, 1, -1), ["b", "a", "c"]);
  assert.deepEqual(moveWedge(ids, 1, 1), ["a", "c", "b"]);
});

test("orders are stored per kind and ring", () => {
  assert.equal(wedgeOrderKey("image", "tools"), "image/tools");
  const orders = { "image/tools": ["crop"] };
  assert.deepEqual(savedOrderFor(orders, "image", "tools"), ["crop"]);
  assert.equal(savedOrderFor(orders, "image", "actions"), undefined);
  assert.equal(savedOrderFor(orders, "pdf", "tools"), undefined);
  assert.equal(savedOrderFor({ "image/tools": [] }, "image", "tools"), undefined);
  assert.equal(savedOrderFor(undefined, "image", "tools"), undefined);
});

test("wedge labels reuse the ring's own wording", () => {
  assert.equal(wedgeLabelKey("grayscale"), "blackWhite");
  assert.equal(wedgeLabelKey("resizeHalf"), "halfSize");
  assert.equal(wedgeLabelKey("pdfOcr"), "ocr");
  assert.equal(wedgeLabelKey("text"), "extractText");
  assert.equal(wedgeLabelKey("compress"), "compress");
  // The wedge strips, so the ring says so rather than offering a lookup.
  assert.equal(wedgeLabelKey("metadata"), "stripMetadata");
});

test("a kind is marked custom once any of its rings is reordered", () => {
  const orders = { "image/tools": ["crop"], "pdf/actions": ["split"] };
  assert.equal(hasCustomOrder(orders, "image"), true);
  assert.equal(hasCustomOrder(orders, "pdf"), true);
  assert.equal(hasCustomOrder(orders, "video"), false);
  assert.equal(hasCustomOrder({ "image/tools": [] }, "image"), false);
});

test("saving is offered only when the draft differs from storage", () => {
  const catalogue = ["compress", "crop", "ocr"];
  assert.equal(isOrderDirty(catalogue, catalogue), false);
  assert.equal(isOrderDirty(["crop", "compress", "ocr"], catalogue), true);
  // Reordering into the same visible sequence is not a change.
  assert.equal(isOrderDirty(["crop", "compress", "ocr"], catalogue, ["crop"]), false);
  assert.equal(isOrderDirty(["crop", "compress"], catalogue, ["crop"]), true);
});

test("the derived action list covers every catalogue wedge", () => {
  // Send is appended by the menu, so it is deliberately absent.
  assert.equal(WEDGE_ACTION_IDS.includes("send"), false);
  for (const kind of ORDERABLE_KINDS) {
    for (const id of defaultWedgeIds("tools", kind, [], true)) {
      assert.equal(WEDGE_ACTION_IDS.includes(id), true, `${kind}/tools ${id}`);
    }
    for (const id of defaultWedgeIds("actions", kind, ["PNG", "PDF"], true)) {
      assert.equal(WEDGE_ACTION_IDS.includes(id), true, `${kind}/actions ${id}`);
    }
  }
});

test("a preset shows the wedges the ring offers for that kind", () => {
  // Convert only appears where a typical source of the kind has targets.
  assert.equal(presetWedgeIds("image", "actions").includes("convert"), true);
  assert.equal(presetWedgeIds("image", "actions").includes("crop"), true);
  assert.equal(presetWedgeIds("video", "tools").includes("extractAudio"), true);
  assert.equal(presetWedgeIds("pdf", "tools").includes("split"), true);
  assert.equal(presetWedgeIds("document", "actions").includes("convert"), true);
  // Merge follows the multi-select rule, so the preset always shows it.
  assert.equal(presetWedgeIds("image", "tools").at(-1), "merge");
  assert.equal(presetWedgeIds("audio", "tools").includes("merge"), false);
  // Send is appended after ordering, so it never appears in the editable list.
  assert.equal(presetWedgeIds("image", "tools").includes("send"), false);
});

test("every kind has a preset for both editable rings", () => {
  for (const kind of ORDERABLE_KINDS) {
    assert.ok(presetWedgeIds(kind, "actions").length > 0, kind);
    assert.ok(presetWedgeIds(kind, "tools").length > 0, kind);
  }
});
