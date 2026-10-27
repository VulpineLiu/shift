import assert from "node:assert/strict";
import { test } from "node:test";

import {
  ORDERABLE_KINDS,
  WEDGE_ACTION_IDS,
  defaultWedgeIds,
  orderWedgeIds,
  savedOrderFor,
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

test("orders are stored per kind and ring", () => {
  assert.equal(wedgeOrderKey("image", "tools"), "image/tools");
  const orders = { "image/tools": ["crop"] };
  assert.deepEqual(savedOrderFor(orders, "image", "tools"), ["crop"]);
  assert.equal(savedOrderFor(orders, "image", "actions"), undefined);
  assert.equal(savedOrderFor(orders, "pdf", "tools"), undefined);
  assert.equal(savedOrderFor({ "image/tools": [] }, "image", "tools"), undefined);
  assert.equal(savedOrderFor(undefined, "image", "tools"), undefined);
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
