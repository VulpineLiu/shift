import { listen } from "@tauri-apps/api/event";
import { isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  Camera,
  Contrast,
  Crop,
  EyeOff,
  File,
  FileAudio,
  FileImage,
  FileOutput,
  FileSpreadsheet,
  FileText,
  FileType,
  FileVideo,
  ExternalLink,
  FlipHorizontal,
  FolderOpen,
  ImagePlus,
  LoaderCircle,
  Merge,
  Minimize2,
  Music,
  PenLine,
  Presentation,
  RotateCw,
  ScanText,
  Scaling,
  Scissors,
  SlidersHorizontal,
  Tags,
  Video,
  Volume1,
  VolumeX,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ComponentType } from "react";
import { api } from "../lib/api";
import { recognizeImage } from "../lib/ocr";
import { basename, formatBytes, recentKind, rememberRecent } from "../lib/utils";
import type { FileInfo, RadialContext } from "../types";
import { useI18n } from "../i18n";

const SIZE = 760;
const CENTER = SIZE / 2;
const OUTER = 310;
const INNER = 128;
const START = -90;

type Ring = "actions" | "formats" | "tools";
type IconType = ComponentType<{ size?: number; strokeWidth?: number; className?: string }>;

interface Wedge {
  id: string;
  label: string;
  icon: IconType;
  format?: string;
}

interface RadialEventPayload {
  x: number;
  y: number;
  mode: "formats" | "tools" | "actions";
}

interface Point {
  x: number;
  y: number;
}

function polar(radius: number, angle: number) {
  const radians = (angle * Math.PI) / 180;
  return {
    x: CENTER + Math.cos(radians) * radius,
    y: CENTER + Math.sin(radians) * radius,
  };
}

function wedgePath(index: number, count: number): string {
  const step = 360 / count;
  const gap = Math.min(2.4, step * 0.06);
  const start = START - step / 2 + index * step + gap / 2;
  const end = START - step / 2 + (index + 1) * step - gap / 2;
  const outerStart = polar(OUTER, start);
  const outerEnd = polar(OUTER, end);
  const innerEnd = polar(INNER, end);
  const innerStart = polar(INNER, start);
  const largeArc = end - start > 180 ? 1 : 0;
  return [
    `M ${outerStart.x} ${outerStart.y}`,
    `A ${OUTER} ${OUTER} 0 ${largeArc} 1 ${outerEnd.x} ${outerEnd.y}`,
    `L ${innerEnd.x} ${innerEnd.y}`,
    `A ${INNER} ${INNER} 0 ${largeArc} 0 ${innerStart.x} ${innerStart.y}`,
    "Z",
  ].join(" ");
}

function sectorAt(x: number, y: number, count: number): number | null {
  const dx = x - CENTER;
  const dy = y - CENTER;
  const distance = Math.hypot(dx, dy);
  if (distance < 92 || distance > OUTER + 34) return null;
  const step = 360 / count;
  const angle = (Math.atan2(dy, dx) * 180) / Math.PI;
  const offset = (angle - (START - step / 2) + 720) % 360;
  return Math.floor(offset / step);
}

function formatIcon(format: string): IconType {
  if (format === "PDF" || format === "TXT" || format === "MD" || format === "DOCX") {
    return FileText;
  }
  return FileImage;
}

function fileGlyph(kind: FileInfo["kind"]): IconType {
  switch (kind) {
    case "image":
      return FileImage;
    case "video":
      return FileVideo;
    case "audio":
      return FileAudio;
    case "pdf":
      return FileType;
    case "document":
      return FileText;
    case "presentation":
      return Presentation;
    case "spreadsheet":
      return FileSpreadsheet;
    default:
      return File;
  }
}

function formatSetFor(kind: FileInfo["kind"], extension: string): string[] {
  switch (kind) {
    case "image":
      return ["PNG", "JPG", "WEBP", "HEIC", "TIFF", "BMP", "GIF", "PDF"];
    case "video":
      return ["MP4", "MOV", "GIF", "M4A"];
    case "audio":
      return ["M4A", "WAV", "MP3"];
    case "pdf":
      return ["PNG", "JPG", "TIFF", "HEIC", "TXT", "MD", "DOCX", "RTF"];
    case "document":
      return ["docx", "rtf", "odt", "txt", "md", "html", "htm"].includes(extension)
        ? ["PDF", "DOCX", "MD", "TXT", "HTML", "RTF", "ODT"]
        : [];
    case "presentation":
      return extension === "pptx" ? ["PDF", "MD"] : [];
    case "spreadsheet":
      return ["xlsx", "xls", "xlsb", "ods", "csv", "tsv"].includes(extension)
        ? ["PDF", "MD"]
        : [];
    default:
      return [];
  }
}

function actionWedge(
  id: string,
  label: string,
  icon: IconType,
  format?: string,
): Wedge {
  return { id, label, icon, format };
}

export function RadialMenu() {
  const { t } = useI18n();
  const [context, setContext] = useState<RadialContext | null>(null);
  const [ring, setRing] = useState<Ring>("actions");
  const [hovered, setHovered] = useState<number | null>(null);
  const [pointer, setPointer] = useState<Point | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const statusTimer = useRef<number | null>(null);
  const hoveredRef = useRef<number | null>(null);
  const ringRef = useRef<Ring>("actions");
  const contextRef = useRef<RadialContext | null>(null);
  const wedgesRef = useRef<Wedge[]>([]);

  useEffect(() => {
    ringRef.current = ring;
  }, [ring]);
  useEffect(() => {
    hoveredRef.current = hovered;
  }, [hovered]);
  useEffect(() => {
    contextRef.current = context;
  }, [context]);

  const kind = context?.fileInfo[0]?.kind ?? "other";
  const formats = formatSetFor(kind, context?.fileInfo[0]?.extension ?? "");

  const actions: Wedge[] =
    kind === "image"
      ? [
          actionWedge("compress", t("compress"), Minimize2),
          actionWedge("metadata", t("metadata"), Tags),
          actionWedge("edit", t("edit"), SlidersHorizontal),
          actionWedge("annotate", t("annotate"), PenLine),
          actionWedge("addBackground", t("addBackground"), ImagePlus),
          actionWedge("crop", t("crop"), Crop),
          actionWedge("redact", t("redact"), EyeOff),
          actionWedge("ocr", t("ocr"), ScanText),
          actionWedge("convert", t("convert"), FileOutput),
          ...(context && context.fileInfo.length > 1
            ? [actionWedge("merge", t("merge"), Merge)]
            : []),
        ]
      : kind === "video" || kind === "audio"
        ? [
            actionWedge("convert", t("convert"), FileOutput),
            actionWedge("compress", t("compress"), Minimize2),
            actionWedge("edit", t("edit"), SlidersHorizontal),
            actionWedge("reveal", t("reveal"), FolderOpen),
          ]
        : kind === "pdf"
          ? [
              actionWedge("convert", t("convert"), FileOutput),
              actionWedge("compress", t("compress"), Minimize2),
              actionWedge("pdfOcr", t("ocr"), ScanText),
              actionWedge("text", t("extractText"), FileText),
              actionWedge("split", t("split"), Scissors),
              actionWedge("rotate", t("rotate"), RotateCw),
              actionWedge("metadata", t("metadata"), Tags),
              actionWedge("reveal", t("reveal"), FolderOpen),
              ...(context && context.fileInfo.length > 1
                ? [actionWedge("merge", t("merge"), Merge)]
                : []),
            ]
          : kind === "presentation" || kind === "spreadsheet" || kind === "document"
            ? [
                ...(formats.length > 0
                  ? [actionWedge("convert", t("convert"), FileOutput)]
                  : []),
                actionWedge("edit", t("edit"), SlidersHorizontal),
                actionWedge("open", t("open"), ExternalLink),
                actionWedge("reveal", t("reveal"), FolderOpen),
              ]
              : [
                  actionWedge("open", t("open"), ExternalLink),
                  actionWedge("reveal", t("reveal"), FolderOpen),
                ];

  const formatWedges: Wedge[] = formats.map((format) =>
    actionWedge(format.toLowerCase(), format, formatIcon(format), format),
  );
  const toolWedges: Wedge[] =
    kind === "image"
      ? [
          actionWedge("rotate", t("rotate"), RotateCw),
          actionWedge("flip", t("flip"), FlipHorizontal),
          actionWedge("grayscale", t("blackWhite"), Contrast),
          actionWedge("resizeHalf", t("halfSize"), Scaling),
          actionWedge("compress", t("compress"), Minimize2),
          actionWedge("metadata", t("metadata"), Tags),
          actionWedge("crop", t("crop"), Crop),
          actionWedge("cutout", t("cutout"), Scissors),
          ...(context && context.fileInfo.length > 1
            ? [actionWedge("merge", t("merge"), Merge)]
            : []),
        ]
      : kind === "video"
        ? [
            actionWedge("compress", t("compress"), Minimize2),
            actionWedge("hd720", t("hd720"), Video),
            actionWedge("mute", t("mute"), VolumeX),
            actionWedge("extractAudio", t("extractAudio"), Music),
            actionWedge("frame", t("frame"), Camera),
            actionWedge("reveal", t("reveal"), FolderOpen),
          ]
        : kind === "audio"
          ? [
              actionWedge("compress", t("compress"), Minimize2),
              actionWedge("mono", t("mono"), Volume1),
              actionWedge("reveal", t("reveal"), FolderOpen),
            ]
          : kind === "pdf"
            ? [
                actionWedge("compress", t("compress"), Minimize2),
                actionWedge("metadata", t("metadata"), Tags),
                actionWedge("rotate", t("rotate"), RotateCw),
                actionWedge("split", t("split"), Scissors),
                actionWedge("text", t("extractText"), FileText),
                actionWedge("reveal", t("reveal"), FolderOpen),
                ...(context && context.fileInfo.length > 1
                  ? [actionWedge("merge", t("merge"), Merge)]
                  : []),
              ]
            : kind === "document"
              ? [
                  actionWedge("plain", t("plain"), FileText),
                  actionWedge("reveal", t("reveal"), FolderOpen),
                ]
              : [
                  actionWedge("open", t("open"), ExternalLink),
                  actionWedge("reveal", t("reveal"), FolderOpen),
                ];
  const wedges =
    ring === "formats" ? formatWedges : ring === "tools" ? toolWedges : actions;
  wedgesRef.current = wedges;
  const wedgeCount = Math.max(wedges.length, 1);
  const hoveredWedge = hovered !== null ? wedges[hovered] : undefined;

  function flash(message: string, duration = 1600) {
    if (statusTimer.current) window.clearTimeout(statusTimer.current);
    setStatus(message);
    statusTimer.current = window.setTimeout(() => setStatus(null), duration);
  }

  function applyContext(next: RadialContext | null) {
    if (!next) return;
    setContext(next);
    let nextRing: Ring =
      next.mode === "formats" ? "formats" : next.mode === "tools" ? "tools" : "actions";
    // Opening the format ring straight from the home panel must never land on an
    // empty wheel, so unsupported types fall back to their action list.
    if (
      nextRing === "formats" &&
      formatSetFor(next.fileInfo[0]?.kind ?? "other", next.fileInfo[0]?.extension ?? "").length === 0
    ) {
      nextRing = "actions";
    }
    setRing(nextRing);
    ringRef.current = nextRing;
    setHovered(null);
    hoveredRef.current = null;
    setPointer(null);
  }

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    if (!isTauri()) {
      const demoMode = new URLSearchParams(window.location.search).get("mode") === "actions"
        ? "actions"
        : "formats";
      applyContext({
        files: ["/Users/demo/tangerine tree.png"],
        fileInfo: [
          {
            path: "/Users/demo/tangerine tree.png",
            name: "tangerine tree.png",
            extension: "png",
            kind: "image",
            size: 2_306_867,
            preview: "/demo-preview.svg",
          },
        ],
        mode: demoMode,
        dragging: false,
      });
      return;
    }
    void api.getRadialContext().then((next) => {
      if (!disposed) applyContext(next);
    });
    void listen<RadialContext>("radial://show", (event) => applyContext(event.payload)).then(
      (unlisten) => unlisteners.push(unlisten),
    );
    void listen<RadialEventPayload>("radial://move", (event) => {
      setPointer({ x: event.payload.x, y: event.payload.y });
      const count = wedgesRef.current.length;
      if (count === 0) return;
      const next = sectorAt(event.payload.x, event.payload.y, count);
      if (next !== hoveredRef.current) {
        hoveredRef.current = next;
        setHovered(next);
      }
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<RadialEventPayload>("radial://drop", (event) => {
      const activeRing = ringRef.current;
      const activeWedges = wedgesRef.current;
      const index = sectorAt(event.payload.x, event.payload.y, Math.max(activeWedges.length, 1));
      if (index === null || index < 0 || index >= activeWedges.length) {
        void api.hideRadial();
        return;
      }
      const wedge = activeWedges[index];
      if (activeRing === "formats") {
        void executeFormat(wedge.id);
      } else {
        void executeAction(wedge.id);
      }
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<"formats" | "tools" | "actions">("radial://mode", (event) => {
      const nextRing: Ring =
        event.payload === "formats" ? "formats" : event.payload === "tools" ? "tools" : "actions";
      ringRef.current = nextRing;
      setRing(nextRing);
      setHovered(null);
      hoveredRef.current = null;
    }).then((unlisten) => unlisteners.push(unlisten));

    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
      if (statusTimer.current) window.clearTimeout(statusTimer.current);
    };
    // Geometry is recomputed from the current catalog; setup only needs to run once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    if (!isTauri() || !/Windows/i.test(navigator.userAgent)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(async (event) => {
        if (disposed) return;
        const payload = event.payload;
        if (payload.type === "leave") {
          void api.hideRadial();
          return;
        }
        if (payload.type === "over") {
          const scale = window.devicePixelRatio || 1;
          const count = wedgesRef.current.length;
          if (count > 0) {
            const next = sectorAt(
              payload.position.x / scale,
              payload.position.y / scale,
              count,
            );
            if (next !== hoveredRef.current) {
              hoveredRef.current = next;
              setHovered(next);
            }
          }
          return;
        }
        if (!payload.paths.length) {
          void api.hideRadial();
          return;
        }
        try {
          const fileInfo = await api.inspectFiles(payload.paths);
          if (disposed || fileInfo.length === 0) return;
          const mode: "formats" | "tools" = ringRef.current === "formats" ? "formats" : "tools";
          const next: RadialContext = {
            files: payload.paths,
            fileInfo,
            mode,
            dragging: true,
          };
          applyContext(next);

          if (payload.type === "drop") {
            const scale = window.devicePixelRatio || 1;
            const activeWedges = wedgesRef.current;
            const index = sectorAt(
              payload.position.x / scale,
              payload.position.y / scale,
              Math.max(activeWedges.length, 1),
            );
            if (index === null || !activeWedges[index]) {
              void api.hideRadial();
            } else if (ringRef.current === "formats") {
              void executeFormat(activeWedges[index].id);
            } else {
              void executeAction(activeWedges[index].id);
            }
          }
        } catch (error) {
          flash(String(error), 2400);
        }
      })
      .then((stop) => {
        unlisten = stop;
      });
    return () => {
      disposed = true;
      unlisten?.();
    };
    // Native drop listeners only need to be installed once per webview.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  async function executeFormat(format: string) {
    const current = contextRef.current;
    if (!current || busy) return;
    const currentKind = current.fileInfo[0]?.kind;
    if (
      currentKind !== "image" &&
      currentKind !== "video" &&
      currentKind !== "audio" &&
      currentKind !== "pdf" &&
      currentKind !== "document" &&
      currentKind !== "presentation" &&
      currentKind !== "spreadsheet"
    ) {
      flash(t("comingSoon"), 1800);
      return;
    }
    setBusy(true);
    flash(t("processing"), 12000);
    try {
      const outcome = await api.performFileAction("convert", current.files, format.toLowerCase());
      for (const output of outcome.outputs) {
        rememberRecent({ path: output, kind: recentKind(output), at: Date.now() });
      }
      flash(outcome.message || t("saved"), 900);
      window.setTimeout(() => void api.hideRadial(), 850);
    } catch (error) {
      flash(String(error), 2600);
      window.setTimeout(() => void api.hideRadial(), 2600);
    } finally {
      setBusy(false);
    }
  }

  async function executeAction(id: string) {
    const current = contextRef.current;
    if (!current || busy) return;
    if (id === "convert") {
      const activeFormats = formatSetFor(
        current.fileInfo[0]?.kind ?? "other",
        current.fileInfo[0]?.extension ?? "",
      );
      if (activeFormats.length === 0) {
        flash(t("unsupported"), 1600);
        return;
      }
      setRing("formats");
      ringRef.current = "formats";
      setHovered(null);
      hoveredRef.current = null;
      flash(t("chooseFormat"), 1200);
      return;
    }

    if (id === "pdfOcr") {
      setBusy(true);
      try {
        const pages = await api.renderPdfPages(current.files[0]);
        const chunks: string[] = [];
        for (let index = 0; index < pages.length; index += 1) {
          flash(
            `${t("ocrRunning")} ${index + 1}/${pages.length}`,
            120000,
          );
          const dataUrl = await api.readFileDataUrl(pages[index]);
          const result = await recognizeImage(dataUrl, (progress) => {
            flash(
              `${t("ocrRunning")} ${index + 1}/${pages.length} · ${Math.round(progress * 100)}%`,
              120000,
            );
          });
          chunks.push(`--- Page ${index + 1} ---\n${result.text}`);
        }
        const output = await api.saveTextNear(current.files[0], chunks.join("\n\n"), " (OCR)");
        rememberRecent({ path: output, kind: "text", at: Date.now() });
        await api.openInEditor(output);
        void api.hideRadial();
      } catch (error) {
        flash(String(error), 3200);
        window.setTimeout(() => void api.hideRadial(), 3200);
      } finally {
        setBusy(false);
      }
      return;
    }

    if (
      ["ocr", "edit", "annotate", "redact", "addBackground", "crop", "cutout"].includes(id)
    ) {
      try {
        await api.openInEditor(current.files[0]);
        void api.hideRadial();
      } catch (error) {
        flash(String(error), 2200);
      }
      return;
    }

    if (id === "extractAudio" || id === "plain") {
      setBusy(true);
      flash(t("processing"), 12000);
      try {
        const outcome = await api.performFileAction(
          "convert",
          current.files,
          id === "extractAudio" ? "m4a" : "txt",
        );
        for (const output of outcome.outputs) {
          rememberRecent({ path: output, kind: recentKind(output), at: Date.now() });
        }
        flash(outcome.message || t("saved"), 900);
        window.setTimeout(() => void api.hideRadial(), 850);
      } catch (error) {
        flash(String(error), 2600);
        window.setTimeout(() => void api.hideRadial(), 2600);
      } finally {
        setBusy(false);
      }
      return;
    }

    const mapping: Record<string, string> = {
      compress: "compress",
      merge: "merge",
      metadata: "stripMetadata",
      rotate: "rotate",
      flip: "flip",
      grayscale: "grayscale",
      resizeHalf: "resizeHalf",
      mute: "mute",
      hd720: "resize720",
      frame: "frame",
      mono: "mono",
      reveal: "reveal",
      split: "split",
      text: "text",
      open: "open",
    };
    const action = mapping[id];
    if (!action) {
      flash(t("comingSoon"));
      return;
    }
    setBusy(true);
    flash(t("processing"), 12000);
    try {
      const outcome = await api.performFileAction(action, current.files);
      for (const output of outcome.outputs) {
        rememberRecent({ path: output, kind: recentKind(output), at: Date.now() });
      }
      flash(outcome.message || t("saved"), 900);
      window.setTimeout(() => void api.hideRadial(), 850);
    } catch (error) {
      flash(String(error), 2600);
      window.setTimeout(() => void api.hideRadial(), 2600);
    } finally {
      setBusy(false);
    }
  }

  const first = context?.fileInfo[0];
  const displayName =
    context && context.fileInfo.length > 1
      ? `${context.fileInfo.length} ${t("files")}`
      : first?.name || "";

  if (!context || context.files.length === 0) {
    return <div className="radial-stage radial-waiting" />;
  }

  return (
    <div
      className={`radial-stage ${ring === "formats" ? "is-formats" : ""}`}
      onPointerDown={(event) => {
        if (event.target === event.currentTarget) void api.hideRadial();
      }}
    >
      <div className="radial-ambient" />
      <div className="radial-glass" />

      <svg
        className="radial-svg"
        width={SIZE}
        height={SIZE}
        viewBox={`0 0 ${SIZE} ${SIZE}`}
        aria-label={ring === "formats" ? t("chooseFormat") : t("chooseAction")}
      >
        <defs>
          <linearGradient id="radialFrame" x1="80" y1="40" x2="680" y2="720">
            <stop offset="0" stopColor="#fff7ef" stopOpacity="0.72" />
            <stop offset="0.52" stopColor="#ffe7d7" stopOpacity="0.64" />
            <stop offset="1" stopColor="#fbd9d8" stopOpacity="0.58" />
          </linearGradient>
          <linearGradient id="radialHot" x1="230" y1="140" x2="540" y2="620">
            <stop offset="0" stopColor="#fffaf5" />
            <stop offset="1" stopColor="#ffe4d2" />
          </linearGradient>
          <filter id="radialShadow" x="-30%" y="-30%" width="160%" height="160%">
            <feDropShadow dx="0" dy="10" stdDeviation="16" floodColor="#7d3b24" floodOpacity="0.18" />
          </filter>
        </defs>
        <circle cx={CENTER} cy={CENTER} r={OUTER + 14} fill="rgba(255,255,255,0.16)" />
        <circle
          cx={CENTER}
          cy={CENTER}
          r={OUTER + 8}
          fill="url(#radialFrame)"
          filter="url(#radialShadow)"
        />
        {wedges.map((wedge, index) => {
          const active = hovered === index;
          return (
            <g
              key={wedge.id}
              className={`radial-wedge ${active ? "is-active" : ""}`}
              onPointerEnter={() => {
                if (!context?.dragging) setHovered(index);
              }}
              onPointerLeave={() => {
                if (!context?.dragging && hovered === index) setHovered(null);
              }}
              onClick={() => {
                if (busy) return;
                if (ring === "formats") void executeFormat(wedge.id);
                else void executeAction(wedge.id);
              }}
            >
              <path
                d={wedgePath(index, wedgeCount)}
                fill={active ? "url(#radialHot)" : "rgba(255,245,237,0.4)"}
                stroke={active ? "rgba(255,255,255,0.8)" : "rgba(190,112,78,0.12)"}
                strokeWidth={active ? 2 : 1}
              />
            </g>
          );
        })}
      </svg>

      {wedges.map((wedge, index) => {
        const step = 360 / wedgeCount;
        const angle = START + index * step;
        const position = polar((OUTER + INNER) / 2 - 2, angle);
        const Icon = wedge.icon;
        return (
          <button
            key={`label-${wedge.id}`}
            className={`radial-label ${ring === "formats" ? "is-format" : ""} ${hovered === index ? "is-active" : ""}`}
            style={{ left: position.x, top: position.y }}
            onPointerEnter={() => {
              if (!context?.dragging) setHovered(index);
            }}
            onPointerLeave={() => {
              if (!context?.dragging && hovered === index) setHovered(null);
            }}
            onClick={() => {
              if (busy) return;
              if (ring === "formats") void executeFormat(wedge.id);
              else void executeAction(wedge.id);
            }}
          >
            {ring === "formats" ? null : (
              <Icon size={wedge.id === "convert" ? 23 : 22} strokeWidth={1.8} />
            )}
            <span>{wedge.label}</span>
          </button>
        );
      })}

      <div className="radial-center">
        {context.fileInfo.length > 1 ? (
          <div
            className={`radial-file-stack stack-${Math.min(context.fileInfo.length, 3)}`}
          >
            {context.fileInfo.slice(0, 3).map((file, index) => {
              const Glyph = fileGlyph(file.kind);
              return (
                <span key={file.path} className={`radial-stack-card card-${index}`}>
                  <Glyph size={34} strokeWidth={1.6} />
                </span>
              );
            })}
          </div>
        ) : first?.preview ? (
          <img className="radial-preview" src={first.preview} alt="" draggable={false} />
        ) : (
          <div className="radial-fallback">
            {(() => {
              const Glyph = fileGlyph(kind);
              return <Glyph size={42} strokeWidth={1.5} />;
            })()}
          </div>
        )}
        <div className="radial-center-info">
          <div className="radial-file-pill" title={displayName}>
            {displayName}
          </div>
          <div className="radial-size-pill">
            {first
              ? formatBytes(context?.fileInfo.reduce((sum, file) => sum + file.size, 0) ?? 0)
              : "—"}
          </div>
        </div>
      </div>

      {context.dragging && pointer && first?.preview ? (
        <div
          className="radial-drag-file"
          style={{
            left: Math.min(SIZE - 150, Math.max(12, pointer.x + 24)),
            top: Math.min(SIZE - 150, Math.max(12, pointer.y + 24)),
          }}
        >
          <img src={first.preview} alt="" draggable={false} />
          <span>{displayName}</span>
        </div>
      ) : null}

      {busy || status || context.dragging ? (
        <div className="radial-hint">
          {busy ? (
            <>
              <LoaderCircle size={15} className="spin" />
              {status || t("processing")}
            </>
          ) : hoveredWedge ? (
            ring === "formats" ? (
              <>
                {t("chooseFormat")}
                <b>{hoveredWedge.label}</b>
              </>
            ) : (
              <>
                {hoveredWedge.label}
                <span>·</span>
                {t("releaseCancel")}
              </>
            )
          ) : status ? (
            status
          ) : ring === "formats" ? (
            t("chooseFormat")
          ) : (
            t("chooseAction")
          )}
        </div>
      ) : null}
    </div>
  );
}
