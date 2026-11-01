import assert from "node:assert/strict";
import { test } from "node:test";

import { outcomeMessage } from "./feedback.ts";

// A dictionary stand-in that shows what a lookup asked for, so the test reads as
// "which words did the interface choose" rather than as a copy of i18n.
const phrase = (key: string, vars?: Record<string, string | number>) =>
  vars ? `${key}:${vars.count}` : key;

test("a named outcome is worded by the interface, not by the backend", () => {
  assert.equal(outcomeMessage("outcomePdfText", 1, phrase), "outcomePdfText");
  assert.equal(outcomeMessage("outcomePdfPages", 12, phrase), "outcomePdfPages:12");
});

test("one output is not counted out loud", () => {
  assert.equal(outcomeMessage("outcomeSaved", 1, phrase), "saved");
  assert.equal(outcomeMessage("outcomeSaved", 4, phrase), "outcomeSaved:4");
});

test("an unknown token falls back instead of being shown as-is", () => {
  // An older backend still sending sentences, or a newer one naming something
  // this build has no words for: neither may reach the screen verbatim.
  assert.equal(outcomeMessage("Saved 1 file.", 1, phrase), "saved");
  assert.equal(outcomeMessage("outcomeFutureThing", 3, phrase), "saved");
});

test("every code the backend may name has words in both languages", async () => {
  // `err*` and `outcome*` keys are looked up by the token the backend raised, so
  // no `t("…")` call names them and a stale one would pass for dead. Read the
  // Rust sources and require each code they raise to be translated.
  const fs = await import("node:fs");
  const path = await import("node:path");
  const root = path.join(process.cwd(), "src-tauri", "src");
  const raised = new Set<string>();
  for (const entry of fs.readdirSync(root)) {
    if (!entry.endsWith(".rs")) continue;
    const text = fs.readFileSync(path.join(root, entry), "utf8");
    for (const match of text.matchAll(/"(err[A-Z]\w*|outcome[A-Z]\w*)"/g)) {
      raised.add(match[1]);
    }
  }
  assert.ok(raised.size > 0, "the backend raises no codes at all");
  // The dictionary is read as text rather than imported: it pulls in React and the
  // Tauri bridge, neither of which has a webview to talk to in a test run.
  const i18n = fs.readFileSync(path.join(process.cwd(), "src", "i18n.ts"), "utf8");
  const blocks = {
    en: i18n.slice(i18n.indexOf("  en: {"), i18n.indexOf("  zh: {")),
    zh: i18n.slice(i18n.indexOf("  zh: {")),
  };
  for (const code of raised) {
    for (const lang of ["en", "zh"] as const) {
      assert.ok(
        new RegExp(`^\\s{4}${code}:`, "m").test(blocks[lang]),
        `${code} is raised by Rust but has no ${lang} wording`,
      );
    }
  }

  // An outcome that is translated but absent from the two sets below still shows
  // something - just the plain "Saved", which quietly loses what happened.
  const here = await import("./feedback.ts");
  for (const code of raised) {
    if (!code.startsWith("outcome")) continue;
    const worded = here.outcomeMessage(code, 3, (key) => (key === "saved" ? "" : key));
    assert.equal(worded, code, `${code} is named by Rust but not worded here`);
  }
});

test("both dictionaries offer exactly the same words", async () => {
  // A key missing from one dictionary falls back to the other in silence, so a
  // Chinese user reads English and nothing appears to have gone wrong. Listing
  // them in one order is what keeps the pair readable, so assert that too.
  const fs = await import("node:fs");
  const path = await import("node:path");
  const i18n = fs.readFileSync(path.join(process.cwd(), "src", "i18n.ts"), "utf8");
  const keys = (block: string) =>
    [...block.matchAll(/^\s{4}([A-Za-z0-9_]+):/gm)].map((match) => match[1]);
  const en = keys(i18n.slice(i18n.indexOf("  en: {"), i18n.indexOf("  zh: {")));
  const zh = keys(i18n.slice(i18n.indexOf("  zh: {")));
  assert.ok(en.length > 0, "neither dictionary was found in the source");
  assert.deepEqual(zh, en, "the two dictionaries list different keys");
});
