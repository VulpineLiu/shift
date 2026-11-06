import assert from "node:assert/strict";
import { test } from "node:test";

import { hitTestDropTarget, releasePoint, toWindowPoint } from "./dropTargets.ts";

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

const viewport = { width: 1080, height: 720 };
const lastOnButton = { x: 342, y: 468 };

test("a reported release point is used as it came", () => {
  assert.deepEqual(releasePoint({ x: 500, y: 300 }, lastOnButton, viewport), { x: 500, y: 300 });
});

test("a release outside the window runs nothing of ours", () => {
  // Dragged out to Explorer or a chat window. The last position inside the window is
  // where the pointer left through, which may well have been over a button, so it must
  // not stand in here.
  assert.equal(releasePoint({ x: 1500, y: 900 }, lastOnButton, viewport), null);
  assert.equal(releasePoint({ x: -5, y: 10 }, lastOnButton, viewport), null);
});

test("a zeroed release point falls back to the pointer's last position", () => {
  // Some engines report (0, 0) on `dragend` rather than the position the drag ended at,
  // which is an absent coordinate rather than a top-left release.
  assert.deepEqual(releasePoint({ x: 0, y: 0 }, lastOnButton, viewport), lastOnButton);
});

test("a zeroed release with nothing observed is still a release at the origin", () => {
  // With no move ever observed there is nothing better to aim at, and (0, 0) is a
  // corner like any other: it simply hits no button.
  assert.deepEqual(releasePoint({ x: 0, y: 0 }, null, viewport), { x: 0, y: 0 });
  assert.equal(hitTestDropTarget([["formats", button]], { x: 0, y: 0 }), null);
});

test("a near miss still counts as aiming at the button", () => {
  assert.equal(hitTestDropTarget([["formats", button]], { x: 95, y: 205 }), "formats");
});
