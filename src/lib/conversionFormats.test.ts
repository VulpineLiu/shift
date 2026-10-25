import assert from "node:assert/strict";
import { test } from "node:test";

import { conversionFormats } from "./conversionFormats.ts";

test("document conversion never offers the source format again", () => {
  const cases = {
    docx: ["PDF", "MD", "TXT", "HTML", "RTF"],
    rtf: ["PDF", "DOCX", "MD", "TXT", "HTML"],
    odt: ["PDF", "DOCX", "MD", "TXT", "HTML", "RTF"],
    txt: ["PDF", "DOCX", "MD", "HTML", "RTF"],
    md: ["PDF", "DOCX", "TXT", "HTML", "RTF"],
    html: ["PDF", "DOCX", "MD", "TXT", "RTF"],
    htm: ["PDF", "DOCX", "MD", "TXT", "RTF"],
  };

  for (const [extension, expected] of Object.entries(cases)) {
    assert.deepEqual(conversionFormats("document", extension), expected, extension);
  }
});

test("ODT is never offered as an output format", () => {
  for (const [kind, extension] of [
    ["document", "docx"],
    ["document", "rtf"],
    ["document", "txt"],
    ["document", "md"],
    ["document", "html"],
    ["pdf", "pdf"],
  ] as const) {
    assert.ok(!conversionFormats(kind, extension).includes("ODT"), extension);
  }
});

test("aliases of the source format are not offered", () => {
  assert.ok(!conversionFormats("image", "jpeg").includes("JPG"));
  assert.ok(!conversionFormats("image", "tif").includes("TIFF"));
  assert.ok(!conversionFormats("image", "heif").includes("HEIC"));
});

test("every convertible image source has real alternative targets", () => {
  for (const extension of [
    "png",
    "jpg",
    "jpeg",
    "webp",
    "tiff",
    "tif",
    "bmp",
    "gif",
    "heic",
    "heif",
    "avif",
  ]) {
    const formats = conversionFormats("image", extension);
    assert.ok(formats.length > 0, extension);
    assert.ok(!formats.some((format) => format.toLowerCase() === extension), extension);
  }
});

test("PDF conversion includes every implemented document target", () => {
  assert.deepEqual(conversionFormats("pdf", "pdf"), [
    "PNG",
    "JPG",
    "TIFF",
    "HEIC",
    "TXT",
    "MD",
    "DOCX",
    "RTF",
  ]);
});

test("media conversion never repeats the source container or codec", () => {
  assert.deepEqual(conversionFormats("video", "mp4"), ["MOV", "GIF", "M4A"]);
  assert.deepEqual(conversionFormats("video", "mov"), ["MP4", "GIF", "M4A"]);
  assert.deepEqual(conversionFormats("audio", "mp3"), ["M4A", "WAV"]);
  assert.deepEqual(conversionFormats("audio", "wav"), ["M4A", "MP3"]);
  assert.deepEqual(conversionFormats("audio", "m4a"), ["WAV", "MP3"]);
});

test("legacy formats stay visible only through open and edit actions", () => {
  assert.deepEqual(conversionFormats("document", "doc"), []);
  assert.deepEqual(conversionFormats("document", "pages"), []);
  assert.deepEqual(conversionFormats("presentation", "ppt"), []);
  assert.deepEqual(conversionFormats("presentation", "key"), []);
  assert.deepEqual(conversionFormats("spreadsheet", "numbers"), []);
});
