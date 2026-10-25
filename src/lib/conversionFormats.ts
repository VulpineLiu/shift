import type { FileInfo } from "../types";

type FileKind = FileInfo["kind"];

const DOCUMENT_SOURCES = new Set(["docx", "rtf", "odt", "txt", "md", "html", "htm"]);
const PRESENTATION_SOURCES = new Set(["pptx"]);
const SPREADSHEET_SOURCES = new Set(["xlsx", "xls", "xlsb", "ods", "csv", "tsv"]);

const IMAGE_FORMATS = ["PNG", "JPG", "WEBP", "HEIC", "TIFF", "BMP", "GIF", "PDF"];
const VIDEO_FORMATS = ["MP4", "MOV", "GIF", "M4A"];
const AUDIO_FORMATS = ["M4A", "WAV", "MP3"];
const PDF_FORMATS = ["PNG", "JPG", "TIFF", "HEIC", "TXT", "MD", "DOCX", "RTF", "ODT"];
const DOCUMENT_FORMATS = ["PDF", "DOCX", "MD", "TXT", "HTML", "RTF", "ODT"];

const FORMAT_ALIASES: Record<string, string> = {
  jpeg: "jpg",
  tif: "tiff",
  heif: "heic",
  htm: "html",
  markdown: "md",
};

function canonicalFormat(extension: string) {
  const normalized = extension.toLowerCase();
  return FORMAT_ALIASES[normalized] ?? normalized;
}

/** Formats offered for a source file, excluding pointless same-format output. */
export function conversionFormats(kind: FileKind, extension: string): string[] {
  let formats: string[];
  switch (kind) {
    case "image":
      formats = IMAGE_FORMATS;
      break;
    case "video":
      formats = VIDEO_FORMATS;
      break;
    case "audio":
      formats = AUDIO_FORMATS;
      break;
    case "pdf":
      formats = PDF_FORMATS;
      break;
    case "document":
      formats = DOCUMENT_SOURCES.has(extension) ? DOCUMENT_FORMATS : [];
      break;
    case "presentation":
      formats = PRESENTATION_SOURCES.has(extension) ? ["PDF", "MD"] : [];
      break;
    case "spreadsheet":
      formats = SPREADSHEET_SOURCES.has(extension) ? ["PDF", "MD"] : [];
      break;
    default:
      formats = [];
  }
  const source = canonicalFormat(extension);
  return formats.filter((format) => canonicalFormat(format) !== source);
}
