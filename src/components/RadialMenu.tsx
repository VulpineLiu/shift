import { listen } from "@tauri-apps/api/event";
import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import {
  CircleAlert,
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
  Send,
  SlidersHorizontal,
  Tags,
  Video,
  Volume1,
  VolumeX,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import type { ComponentType } from "react";
import { api } from "../lib/api";
import { conversionFormats } from "../lib/conversionFormats";
import {
  WEDGE_ACTION_IDS,
  defaultWedgeIds,
  orderWedgeIds,
  savedOrderFor,
  type WedgeOrders,
} from "../lib/radialOrder";
import { recognizeImage } from "../lib/ocr";
import { formatBytes } from "../lib/utils";
import type { FileInfo, RadialContext } from "../types";
import { useI18n } from "../i18n";
import { SharePanel } from "./SharePanel";

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

/**
 * Icon for every wedge the rings can show. Labels map onto the catalogue ids in
 * `src/lib/radialOrder.ts`, which is the single source for the wedge rotation.
 */
const WEDGE_ICONS: Record<string, IconType> = {
  addBackground: ImagePlus,
  annotate: PenLine,
  compress: Minimize2,
  convert: FileOutput,
  crop: Crop,
  cutout: Scissors,
  edit: SlidersHorizontal,
  extractAudio: Music,
  flip: FlipHorizontal,
  frame: Camera,
  grayscale: Contrast,
  hd720: Video,
  merge: Merge,
  metadata: Tags,
  mono: Volume1,
  mute: VolumeX,
  ocr: ScanText,
  open: ExternalLink,
  pdfOcr: ScanText,
  plain: FileText,
  redact: EyeOff,
  resizeHalf: Scaling,
  reveal: FolderOpen,
  rotate: RotateCw,
  send: Send,
  split: Scissors,
  text: FileText,
};

// A wedge whose icon was forgotten renders the generic file glyph, which is easy to add
// by accident and hard to notice, so name it on the console. Empty in a healthy build.
const wedgesWithoutIcons = WEDGE_ACTION_IDS.filter((id) => !WEDGE_ICONS[id]);
if (wedgesWithoutIcons.length > 0) {
  console.warn("radial wedges without an icon:", wedgesWithoutIcons);
}

const WEDGE_LABEL_KEYS: Record<string, string> = {
  grayscale: "blackWhite",
  resizeHalf: "halfSize",
  pdfOcr: "ocr",
  text: "extractText",
  metadata: "stripMetadata",
};

function wedgeLabelKey(id: string): string {
  return WEDGE_LABEL_KEYS[id] ?? id;
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

function actionWedge(
  id: string,
  label: string,
  icon: IconType,
  format?: string,
): Wedge {
  return { id, label, icon, format };
}

/**
 * Turn one wedge id from the catalogue into what the ring renders. An id that names
 * one of the source's convert targets is a format wedge; everything else is an action
 * whose label and icon come from the shared wedge tables.
 */
function buildWedge(
  id: string,
  formats: string[],
  t: (key: string) => string,
): Wedge {
  const target = formats.find((format) => format.toLowerCase() === id);
  if (target) {
    return actionWedge(id, target, formatIcon(target), target);
  }
  return actionWedge(id, t(wedgeLabelKey(id)), WEDGE_ICONS[id] ?? File);
}

function messageOf(error: unknown) {
  const message = error instanceof Error ? error.message : String(error);
  return message.replace(/^Error:\s*/, "") || "Unknown error";
}

function waitForPaint() {
  return new Promise<void>((resolve) => {
    requestAnimationFrame(() => {
      requestAnimationFrame(() => {
        window.setTimeout(resolve, 0);
      });
    });
  });
}

export function RadialMenu() {
  const { t } = useI18n();
  const [context, setContext] = useState<RadialContext | null>(null);
  const [ring, setRing] = useState<Ring>("actions");
  const [hovered, setHovered] = useState<number | null>(null);
  const [pointer, setPointer] = useState<Point | null>(null);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  // Files waiting for a peer to be picked; the ring stays open behind the panel.
  const [sharePaths, setSharePaths] = useState<string[] | null>(null);
  // Custom wedge order per kind and ring, as saved in Settings.
  const [orders, setOrders] = useState<WedgeOrders>({});
  const sharePathsRef = useRef<string[] | null>(null);
  // True once the send panel has reported an outcome; the ring then stays open
  // just long enough to show the message.
  const shareNoticeRef = useRef(false);
  const statusTimer = useRef<number | null>(null);
  const hoveredRef = useRef<number | null>(null);
  const ringRef = useRef<Ring>("actions");
  const contextRef = useRef<RadialContext | null>(null);
  const wedgesRef = useRef<Wedge[]>([]);
  // Whether WebView2's OLE drag has contributed to the current Windows gesture.
  const oleSeenRef = useRef(false);
  // Whether OLE finished this gesture (drop or leave), so a release-based safety
  // net knows to stand down. Cleared when the poller opens the next ring.
  const oleSettledRef = useRef(false);
  const releaseHideRef = useRef<number | null>(null);

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
  const formats = conversionFormats(kind, context?.fileInfo[0]?.extension ?? "");
  const multi = (context?.fileInfo.length ?? 0) > 1;
  // The catalogue decides which wedges exist, the saved order where each appears.
  const ids = defaultWedgeIds(ring, kind, formats, multi);
  const wedges = orderWedgeIds(ids, savedOrderFor(orders, kind, ring)).map((id) =>
    buildWedge(id, formats, t),
  );
  // Sending works for any file, so the wedge joins every type's tool ring.
  if (ring === "tools") wedges.push(actionWedge("send", t("sendShort"), Send));
  wedgesRef.current = wedges;
  const wedgeCount = Math.max(wedges.length, 1);
  const hoveredWedge = hovered !== null ? wedges[hovered] : undefined;

  function flash(message: string, duration = 1600) {
    if (statusTimer.current) window.clearTimeout(statusTimer.current);
    setError(null);
    setStatus(message);
    statusTimer.current = window.setTimeout(() => setStatus(null), duration);
  }

  function showError(reason: unknown) {
    if (statusTimer.current) window.clearTimeout(statusTimer.current);
    setStatus(null);
    setError(messageOf(reason));
  }

  function applyContext(next: RadialContext | null) {
    if (!next) return;
    if (statusTimer.current) window.clearTimeout(statusTimer.current);
    setStatus(null);
    setError(null);
    setSharePaths(null);
    sharePathsRef.current = null;
    shareNoticeRef.current = false;
    setContext(next);
    contextRef.current = next;
    let nextRing: Ring =
      next.mode === "formats" ? "formats" : next.mode === "tools" ? "tools" : "actions";
    // Opening the format ring straight from the home panel must never land on an
    // empty wheel, so unsupported types fall back to their action list.
    if (
      next.files.length > 0 &&
      nextRing === "formats" &&
      conversionFormats(
        next.fileInfo[0]?.kind ?? "other",
        next.fileInfo[0]?.extension ?? "",
      ).length === 0
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
      const requested = new URLSearchParams(window.location.search).get("mode");
      const demoMode = requested === "actions" || requested === "tools" ? requested : "formats";
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
    // Saved wedge orders still apply to builds upgraded from a version that had an
    // in-app editor, so the ring reads them once when this window opens.
    void api.getWedgeOrders().then((next) => {
      if (!disposed) setOrders(next);
    });
    void api.getRadialContext().then((next) => {
      if (!disposed) applyContext(next);
    });
    void listen<RadialContext>("radial://show", (event) => {
      // The poller opens a fresh ring at the start of every gesture, which is the
      // boundary for "has OLE said anything about this drag yet?".
      if (event.payload.dragging) {
        oleSeenRef.current = false;
        oleSettledRef.current = false;
      }
      applyContext(event.payload);
    }).then((unlisten) => unlisteners.push(unlisten));
    // The native poller saw the mouse button go up. OLE reports a drop *after*
    // release, so this must only close a ring that OLE never touched; otherwise a
    // real drop would be hidden before it is delivered.
    void listen("radial://dragend", () => {
      if (!contextRef.current?.dragging || oleSettledRef.current) return;
      // A picker is open on top of the ring; the gesture ending must not close it.
      if (sharePathsRef.current) return;
      // OLE never spoke: nothing is coming, so close immediately. If it did speak,
      // give it a moment to deliver the pending drop before assuming it went silent.
      const delay = oleSeenRef.current ? 400 : 0;
      if (releaseHideRef.current) window.clearTimeout(releaseHideRef.current);
      releaseHideRef.current = window.setTimeout(() => {
        releaseHideRef.current = null;
        if (!oleSettledRef.current && contextRef.current?.dragging) void api.hideRadial();
      }, delay);
    }).then((unlisten) => unlisteners.push(unlisten));
    void listen<RadialEventPayload>("radial://move", (event) => {
      if (sharePathsRef.current) return;
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
      // The picker already opened during the drag; the release that ends the
      // gesture must not be read as a wedge choice.
      if (sharePathsRef.current) return;
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
      if (releaseHideRef.current) window.clearTimeout(releaseHideRef.current);
    };
    // Geometry is recomputed from the current catalog; setup only needs to run once.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      // Finder owns Escape during its own drag session; only the ring opened
      // from a completed drop on the main panel should handle it.
      if (event.key !== "Escape" || contextRef.current?.dragging) return;
      // While the send picker is open its own Escape handler owns the key.
      if (sharePathsRef.current) return;
      event.preventDefault();
      event.stopPropagation();
      void api.hideRadial();
    }
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, []);

  useEffect(() => {
    if (!isTauri() || !/Windows/i.test(navigator.userAgent)) return;
    let disposed = false;
    let unlisten: (() => void) | undefined;
    void getCurrentWebview()
      .onDragDropEvent(async (event) => {
        if (disposed) return;
        const payload = event.payload;
        // OLE owns this gesture now, so the poller's release signal must not
        // hide the ring: OLE reports the drop after the button is already up.
        oleSeenRef.current = true;
        if (payload.type === "leave") {
          oleSettledRef.current = true;
          if (!sharePathsRef.current) void api.hideRadial();
          return;
        }
        if (payload.type === "over") {
          if (sharePathsRef.current) return;
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
          oleSettledRef.current = true;
          if (!sharePathsRef.current) void api.hideRadial();
          return;
        }
        // A drop is terminal even while inspectFiles is still running. Mark it
        // before awaiting so the release safety timer cannot hide the ring.
        if (payload.type === "drop") oleSettledRef.current = true;
        // The picker opened mid-gesture, so this drop has no wedge to run.
        if (sharePathsRef.current) return;
        try {
          const fileInfo = await api.inspectFiles(payload.paths);
          if (disposed) return;
          if (fileInfo.length === 0) {
            oleSettledRef.current = true;
            return;
          }
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
    await waitForPaint();
    try {
      const outcome = await api.performFileAction(
        "convert",
        current.files,
        format.toLowerCase(),
        (done, total, packaging) => {
          flash(
            packaging
              ? t("fileActionPackaging")
              : t("fileActionPageProgress", { done, total }),
            120000,
          );
        },
      );
      flash(outcome.message || t("saved"), 900);
      window.setTimeout(() => void api.hideRadial(), 850);
    } catch (error) {
      showError(error);
    } finally {
      setBusy(false);
    }
  }

  async function executeAction(id: string) {
    const current = contextRef.current;
    if (!current || busy) return;
    if (id === "send") {
      setSharePaths(current.files);
      sharePathsRef.current = current.files;
      shareNoticeRef.current = false;
      return;
    }
    if (id === "convert") {
      const activeFormats = conversionFormats(
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
      flash(t("processing"), 120000);
      await waitForPaint();
      try {
        const pages = await api.renderPdfPages(current.files[0]);
        const chunks: string[] = [];
        try {
          for (let index = 0; index < pages.length; index += 1) {
            flash(
              `${t("ocrRunning")} ${index + 1}/${pages.length}`,
              120000,
            );
            const result = await recognizeImage(convertFileSrc(pages[index]), (progress) => {
              flash(
                `${t("ocrRunning")} ${index + 1}/${pages.length} · ${Math.round(progress * 100)}%`,
                120000,
              );
            });
            chunks.push(`--- Page ${index + 1} ---\n${result.text}`);
          }
        } finally {
          void api.cleanupPdfPages(pages).catch(() => undefined);
        }
        const output = await api.saveTextNear(current.files[0], chunks.join("\n\n"), " (OCR)");
        await api.openInEditor(output);
        void api.hideRadial();
      } catch (error) {
        showError(error);
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
      await waitForPaint();
      try {
        const outcome = await api.performFileAction(
          "convert",
          current.files,
          id === "extractAudio" ? "m4a" : "txt",
        );
        flash(outcome.message || t("saved"), 900);
        window.setTimeout(() => void api.hideRadial(), 850);
      } catch (error) {
        showError(error);
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
    await waitForPaint();
    try {
      const outcome = await api.performFileAction(action, current.files);
      flash(outcome.message || t("saved"), 900);
      window.setTimeout(() => void api.hideRadial(), 850);
    } catch (error) {
      showError(error);
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
      onPointerMove={(event) => {
        if (context.dragging || busy || sharePaths) return;
        const bounds = event.currentTarget.getBoundingClientRect();
        const next = sectorAt(
          event.clientX - bounds.left,
          event.clientY - bounds.top,
          wedgeCount,
        );
        if (next !== hoveredRef.current) {
          hoveredRef.current = next;
          setHovered(next);
        }
      }}
      onPointerLeave={() => {
        if (context.dragging) return;
        hoveredRef.current = null;
        setHovered(null);
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

      <div className={`radial-center ${busy ? "is-busy" : ""} ${error ? "has-error" : ""}`}>
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
        {busy ? (
          <div
            className="radial-progress"
            role="status"
            aria-live="polite"
            aria-label={status || t("processing")}
          >
            <span className="radial-progress-ring" aria-hidden="true" />
            <div className="radial-progress-content">
              <LoaderCircle size={22} className="spin" aria-hidden="true" />
              <strong>{status || t("processing")}</strong>
            </div>
          </div>
        ) : error ? (
          <div className="radial-error" role="alert">
            <CircleAlert size={24} strokeWidth={1.8} aria-hidden="true" />
            <strong>{t("actionFailed")}</strong>
            <span className="radial-error-message">{error}</span>
            <button type="button" onClick={() => setError(null)} aria-label={t("close")}>
              <X size={14} />
            </button>
          </div>
        ) : null}
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

      {!error && !busy && !sharePaths && (status || context.dragging) ? (
        <div className="radial-hint">
          {hoveredWedge ? (
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

      {sharePaths ? (
        <SharePanel
          paths={sharePaths}
          onClose={() => {
            setSharePaths(null);
            sharePathsRef.current = null;
            // A result is flashed inside the ring, so the ring has to survive
            // long enough to show it; a plain close dismisses at once.
            const delay = shareNoticeRef.current ? 2_800 : 0;
            window.setTimeout(() => void api.hideRadial(), delay);
          }}
          onNotice={(message) => {
            shareNoticeRef.current = true;
            flash(message, 2_600);
          }}
        />
      ) : null}
    </div>
  );
}
