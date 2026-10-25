import assert from "node:assert/strict";
import { test } from "node:test";

import { getLocalRecentItems, takeLegacyRecentItems, clearLegacyRecentItems } from "./utils.ts";

/** Minimal store stand-in, installed before the module reads it. */
function withStore(items: Record<string, string | null>, run: () => void) {
  const store = new Map(Object.entries(items));
  const previous = Object.getOwnPropertyDescriptor(globalThis, "localStorage");
  Object.defineProperty(globalThis, "localStorage", {
    configurable: true,
    value: {
      getItem: (key: string) => store.get(key) ?? null,
      setItem: (key: string, value: string) => void store.set(key, value),
      removeItem: (key: string) => void store.delete(key),
    },
  });
  try {
    run();
  } finally {
    if (previous) Object.defineProperty(globalThis, "localStorage", previous);
    else delete (globalThis as Record<string, unknown>).localStorage;
  }
}

test("an empty store yields nothing", () => {
  withStore({}, () => {
    assert.deepEqual(takeLegacyRecentItems(), []);
    assert.deepEqual(getLocalRecentItems(), []);
  });
});

test("the 0.1 rows are read whole, including paths outside the eight it displayed", () => {
  const rows = JSON.stringify([
    { path: "/tmp/a.png", kind: "image", at: 5 },
    { path: "/tmp/b.pdf", kind: "file", at: 4 },
  ]);
  withStore({ "shift.recent": rows }, () => {
    assert.deepEqual(takeLegacyRecentItems(), [
      { path: "/tmp/a.png", kind: "image", at: 5 },
      { path: "/tmp/b.pdf", kind: "file", at: 4 },
    ]);
  });
});

test("clearing the legacy key makes the handoff a one-shot", () => {
  withStore({ "shift.recent": JSON.stringify([{ path: "/tmp/a.png", kind: "image", at: 5 }]) }, () => {
    assert.equal(takeLegacyRecentItems().length, 1);
    clearLegacyRecentItems();
    assert.deepEqual(takeLegacyRecentItems(), []);
  });
});

test("junk in the store is ignored rather than imported", () => {
  withStore({ "shift.recent": "[null,{\"path\":\"\"},{\"path\":\"/tmp/ok.png\",\"at\":1}]" }, () => {
    assert.deepEqual(takeLegacyRecentItems(), [{ path: "/tmp/ok.png", at: 1 }]);
  });
  withStore({ "shift.recent": "{not json" }, () => {
    assert.deepEqual(takeLegacyRecentItems(), []);
  });
  withStore({ "shift.recent": "{\"path\":\"/tmp/a.png\"}" }, () => {
    assert.deepEqual(takeLegacyRecentItems(), []);
  });
});
