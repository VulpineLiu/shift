import assert from "node:assert/strict";
import { test } from "node:test";

import { dragOutPayload, fileUrl } from "./dragOut.ts";

test("unix paths become file URLs with separators kept", () => {
  assert.equal(fileUrl("/Users/me/Shift/photo.png"), "file:///Users/me/Shift/photo.png");
});

test("spaces and unicode are percent-encoded", () => {
  assert.equal(
    fileUrl("/Users/me/My Shift/截图 1.png"),
    "file:///Users/me/My%20Shift/%E6%88%AA%E5%9B%BE%201.png",
  );
});

test("windows paths keep their drive letter", () => {
  assert.equal(fileUrl("C:\\Users\\me\\Shift\\photo.png"), "file:///C:/Users/me/Shift/photo.png");
  assert.equal(fileUrl("C:\\Users\\me\\a b.pdf"), "file:///C:/Users/me/a%20b.pdf");
});

test("repeated separators collapse", () => {
  assert.equal(fileUrl("/Users//me///x.png"), "file:///Users/me/x.png");
});

test("a row is offered as uri-list, plain path and share paths", () => {
  const payload = dragOutPayload("/tmp/a.png");
  assert.deepEqual(payload, {
    "text/uri-list": "file:///tmp/a.png",
    "text/plain": "/tmp/a.png",
    "application/x-shift-share-paths": JSON.stringify(["/tmp/a.png"]),
  });
});

test("unc shares are readable too", () => {
  assert.equal(fileUrl("\\\\server\\share\\a.pdf"), "file:///server/share/a.pdf");
});
