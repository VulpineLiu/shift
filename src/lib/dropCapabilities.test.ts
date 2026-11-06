import assert from "node:assert/strict";
import { test } from "node:test";

import {
  actionCanServe,
  convertible,
  dropEffectFor,
  kindFromPath,
  ocrCanRead,
} from "./dropCapabilities.ts";

test("the classifier matches the backend's kind tables", () => {
  assert.equal(kindFromPath("C:\\Users\\me\\photo.HEIC"), "image");
  assert.equal(kindFromPath("/tmp/clip.mp4"), "video");
  assert.equal(kindFromPath("/tmp/voice.m4a"), "audio");
  assert.equal(kindFromPath("/tmp/report.pdf"), "pdf");
  assert.equal(kindFromPath("/tmp/notes.pages"), "document");
  assert.equal(kindFromPath("/tmp/deck.key"), "presentation");
  assert.equal(kindFromPath("/tmp/sheet.numbers"), "spreadsheet");
  assert.equal(kindFromPath("/tmp/archive.zip"), "other");
  assert.equal(kindFromPath("README"), "other");
});

test("only images reach OCR", () => {
  assert.ok(ocrCanRead(["/tmp/a.png", "C:\\tmp\\b.jpg"]));
  assert.ok(!ocrCanRead(["/tmp/a.pdf"]));
  // One unreadable file in a batch spends the button for the whole batch.
  assert.ok(!ocrCanRead(["/tmp/a.png", "/tmp/b.txt"]));
  assert.ok(!ocrCanRead([]));
});

test("a source with no conversion target spends convert", () => {
  assert.ok(convertible(["/tmp/report.pdf"]));
  assert.ok(!convertible(["/tmp/deck.key"]));
  assert.ok(!convertible(["/tmp/archive.zip"]));
  // A file that cannot convert spends the button for a mixed batch too.
  assert.ok(!convertible(["/tmp/deck.key", "/tmp/a.png"]));
});

test("the tool ring serves any file", () => {
  for (const paths of [["/tmp/a.png"], ["/tmp/x.zip"], ["/tmp/deck.key"], []]) {
    assert.ok(actionCanServe("tools", paths));
  }
});

test("an unknown payload is not a refusal", () => {
  // Spending every button because a drag reported no paths would disable a gesture
  // that does work, so "nothing known yet" leaves all three candidates lit.
  for (const action of ["formats", "tools", "ocr"] as const) {
    assert.ok(actionCanServe(action, null));
    assert.ok(actionCanServe(action, []));
  }
});

test("a PDF converts but never reaches the OCR button", () => {
  assert.ok(actionCanServe("formats", ["/tmp/report.pdf"]));
  assert.ok(actionCanServe("tools", ["/tmp/report.pdf"]));
  assert.ok(!actionCanServe("ocr", ["/tmp/report.pdf"]));
});

test("the drag cursor refuses a button that cannot serve the files", () => {
  assert.equal(dropEffectFor("ocr", ["/tmp/report.pdf"]), "none");
  assert.equal(dropEffectFor("formats", ["/tmp/report.pdf"]), "copy");
  assert.equal(dropEffectFor("tools", ["/tmp/report.pdf"]), "copy");
  assert.equal(dropEffectFor("formats", ["/tmp/deck.key"]), "none");
  assert.equal(dropEffectFor("ocr", ["/tmp/a.png"]), "copy");
  // Nothing under the pointer is not a drop target either.
  assert.equal(dropEffectFor(null, ["/tmp/report.pdf"]), "none");
  // A payload that is not known yet is not a refusal.
  assert.equal(dropEffectFor("ocr", null), "copy");
});
