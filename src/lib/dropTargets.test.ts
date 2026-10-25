import assert from "node:assert/strict";
import { test } from "node:test";

import { hitTestDropTarget, toWindowPoint } from "./dropTargets.ts";

const button = { left: 100, top: 200, right: 300, bottom: 260 };

test("macOS drag positions are already webview points", () => {
  assert.deepEqual(toWindowPoint({ x: 120, y: 220 }, "mac", 2), { x: 120, y: 220 });
});

test("other platforms divide out the device pixel ratio only", () => {
  // The position is measured from the webview corner, so nothing is subtracted.
  assert.deepEqual(toWindowPoint({ x: 240, y: 440 }, "other", 2), { x: 120, y: 220 });
  assert.deepEqual(toWindowPoint({ x: 240, y: 440 }, "other", 0), { x: 240, y: 440 });
});

test("a point on a button selects it", () => {
  assert.equal(
    hitTestDropTarget(
      [
        ["formats", { left: 10, top: 10, right: 90, bottom: 50 }],
        ["tools", button],
        ["ocr", null],
      ],
      { x: 200, y: 230 },
    ),
    "tools",
  );
});

test("a point on no button selects nothing", () => {
  assert.equal(
    hitTestDropTarget(
      [
        ["formats", { left: 10, top: 10, right: 90, bottom: 50 }],
        ["tools", button],
      ],
      { x: 500, y: 500 },
    ),
    null,
  );
});

test("a near miss still counts as aiming at the button", () => {
  assert.equal(hitTestDropTarget([["formats", button]], { x: 95, y: 205 }), "formats");
});
