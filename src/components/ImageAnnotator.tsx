import { writeImage } from "@tauri-apps/plugin-clipboard-manager";
import { isTauri } from "@tauri-apps/api/core";
import {
  ArrowUpRight,
  Check,
  CircleCheck,
  CircleX,
  Copy,
  Crop,
  Droplets,
  EyeOff,
  Grid3X3,
  Highlighter,
  ImagePlus,
  ListOrdered,
  LoaderCircle,
  Save,
  Scissors,
  Square,
  Star,
  Type,
  Undo2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";

type Tool =
  | "rectangle"
  | "arrow"
  | "highlight"
  | "redact"
  | "blur"
  | "pixelate"
  | "number"
  | "crop"
  | "text"
  | "stamp";

/// Stamps are vector glyphs so they stay sharp at any canvas resolution.
export type Stamp = "check" | "cross" | "star";

interface Annotation {
  tool: Tool;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  text?: string;
  number?: number;
  stamp?: Stamp;
}

interface Point {
  x: number;
  y: number;
}

interface ImageSnapshot {
  src: string;
  annotations: Annotation[];
}

interface Props {
  path: string;
  onSaved: (path: string) => void;
}

/// Soften a region by downscaling it and painting it back blurred. Unlike
/// pixelate, which is deliberately blocky, this keeps the shapes readable while
/// removing detail, and it is baked into the output rather than a translucent
/// overlay, so a blurred area cannot be recovered from the saved PNG.
export function drawBlur(
  context: CanvasRenderingContext2D,
  left: number,
  top: number,
  boxWidth: number,
  boxHeight: number,
) {
  const source = document.createElement("canvas");
  source.width = Math.max(1, Math.round(boxWidth));
  source.height = Math.max(1, Math.round(boxHeight));
  const sourceContext = source.getContext("2d");
  if (!sourceContext) return;
  sourceContext.drawImage(context.canvas, left, top, boxWidth, boxHeight, 0, 0, source.width, source.height);

  const factor = 10;
  const small = document.createElement("canvas");
  small.width = Math.max(1, Math.round(source.width / factor));
  small.height = Math.max(1, Math.round(source.height / factor));
  const smallContext = small.getContext("2d");
  if (!smallContext) return;
  smallContext.imageSmoothingEnabled = true;
  smallContext.drawImage(source, 0, 0, small.width, small.height);

  context.save();
  context.beginPath();
  context.rect(left, top, boxWidth, boxHeight);
  context.clip();
  context.imageSmoothingEnabled = true;
  // The filter widens the soft edge the downscale alone leaves behind.
  context.filter = `blur(${Math.max(2, Math.round(Math.min(boxWidth, boxHeight) / 24))}px)`;
  context.drawImage(small, 0, 0, small.width, small.height, left, top, boxWidth, boxHeight);
  context.restore();
}

function stampPath(context: CanvasRenderingContext2D, stamp: Stamp, radius: number) {
  context.beginPath();
  if (stamp === "check") {
    context.lineWidth = Math.max(4, radius * 0.34);
    context.moveTo(-radius * 0.46, radius * 0.04);
    context.lineTo(-radius * 0.12, radius * 0.42);
    context.lineTo(radius * 0.5, -radius * 0.4);
    context.stroke();
    return;
  }
  if (stamp === "cross") {
    context.lineWidth = Math.max(4, radius * 0.3);
    const arm = radius * 0.42;
    context.moveTo(-arm, -arm);
    context.lineTo(arm, arm);
    context.moveTo(arm, -arm);
    context.lineTo(-arm, arm);
    context.stroke();
    return;
  }
  const points = 5;
  for (let index = 0; index < points * 2; index += 1) {
    const outer = index % 2 === 0 ? radius * 0.62 : radius * 0.26;
    const angle = (Math.PI / points) * index - Math.PI / 2;
    const x = Math.cos(angle) * outer;
    const y = Math.sin(angle) * outer;
    if (index === 0) context.moveTo(x, y);
    else context.lineTo(x, y);
  }
  context.closePath();
  context.fill();
}

/// A filled disc plus a glyph, centred on the tap and sized from the canvas so a
/// stamp stays proportional on a 4K screenshot and a small crop alike.
function drawStamp(context: CanvasRenderingContext2D, annotation: Annotation, width: number) {
  const stamp = annotation.stamp ?? "check";
  const radius = Math.max(18, Math.round(width / 22));
  context.save();
  context.translate(annotation.x1, annotation.y1);
  context.fillStyle = stamp === "cross" ? "#e0342b" : stamp === "star" ? "#f5a623" : "#1aa260";
  context.beginPath();
  context.arc(0, 0, radius, 0, Math.PI * 2);
  context.fill();
  context.strokeStyle = "rgba(255,255,255,0.94)";
  context.fillStyle = "#fffdf9";
  context.lineCap = "round";
  context.lineJoin = "round";
  stampPath(context, stamp, radius);
  context.restore();
}

function drawAnnotation(
  context: CanvasRenderingContext2D,
  annotation: Annotation,
  width: number,
) {
  const lineWidth = Math.max(3, Math.round(width / 420));
  const left = Math.min(annotation.x1, annotation.x2);
  const top = Math.min(annotation.y1, annotation.y2);
  const boxWidth = Math.abs(annotation.x2 - annotation.x1);
  const boxHeight = Math.abs(annotation.y2 - annotation.y1);
  context.save();
  context.lineCap = "round";
  context.lineJoin = "round";
  context.lineWidth = lineWidth;
  if (annotation.tool === "rectangle") {
    context.strokeStyle = "#ff5a2f";
    context.strokeRect(left, top, boxWidth, boxHeight);
  } else if (annotation.tool === "highlight") {
    context.fillStyle = "rgba(255, 213, 59, 0.42)";
    context.fillRect(left, top, boxWidth, boxHeight);
  } else if (annotation.tool === "redact") {
    context.fillStyle = "#111111";
    context.fillRect(left, top, boxWidth, boxHeight);
  } else if (annotation.tool === "blur" && boxWidth > 0 && boxHeight > 0) {
    drawBlur(context, left, top, boxWidth, boxHeight);
  } else if (annotation.tool === "stamp" && annotation.stamp) {
    drawStamp(context, annotation, width);
  } else if (annotation.tool === "pixelate" && boxWidth > 0 && boxHeight > 0) {
    const sampleWidth = Math.max(1, Math.round(boxWidth / 12));
    const sampleHeight = Math.max(1, Math.round(boxHeight / 12));
    const sample = document.createElement("canvas");
    sample.width = sampleWidth;
    sample.height = sampleHeight;
    const sampleContext = sample.getContext("2d");
    if (sampleContext) {
      sampleContext.drawImage(
        context.canvas,
        left,
        top,
        boxWidth,
        boxHeight,
        0,
        0,
        sampleWidth,
        sampleHeight,
      );
      context.save();
      context.beginPath();
      context.rect(left, top, boxWidth, boxHeight);
      context.clip();
      context.imageSmoothingEnabled = false;
      context.drawImage(sample, left, top, boxWidth, boxHeight);
      context.restore();
    }
  } else if (annotation.tool === "number") {
    const radius = Math.max(16, Math.round(width / 55));
    context.fillStyle = "#f04f24";
    context.beginPath();
    context.arc(annotation.x1, annotation.y1, radius, 0, Math.PI * 2);
    context.fill();
    context.fillStyle = "#fffdf9";
    context.font = `700 ${Math.round(radius * 1.05)}px -apple-system, BlinkMacSystemFont, sans-serif`;
    context.textAlign = "center";
    context.textBaseline = "middle";
    context.fillText(String(annotation.number ?? 1), annotation.x1, annotation.y1);
  } else if (annotation.tool === "crop") {
    context.strokeStyle = "#fff4e8";
    context.setLineDash([10, 7]);
    context.strokeRect(left, top, boxWidth, boxHeight);
  } else if (annotation.tool === "text" && annotation.text) {
    const fontSize = Math.max(18, Math.round(width / 32));
    context.font = `600 ${fontSize}px -apple-system, BlinkMacSystemFont, sans-serif`;
    context.textBaseline = "top";
    context.lineWidth = Math.max(3, fontSize / 8);
    context.strokeStyle = "rgba(255,255,255,0.92)";
    context.fillStyle = "#f04f24";
    context.strokeText(annotation.text, annotation.x1, annotation.y1);
    context.fillText(annotation.text, annotation.x1, annotation.y1);
  } else {
    context.strokeStyle = "#ff5a2f";
    context.beginPath();
    context.moveTo(annotation.x1, annotation.y1);
    context.lineTo(annotation.x2, annotation.y2);
    context.stroke();
    const angle = Math.atan2(annotation.y2 - annotation.y1, annotation.x2 - annotation.x1);
    const head = Math.max(12, lineWidth * 5);
    context.beginPath();
    context.moveTo(annotation.x2, annotation.y2);
    context.lineTo(
      annotation.x2 - head * Math.cos(angle - Math.PI / 7),
      annotation.y2 - head * Math.sin(angle - Math.PI / 7),
    );
    context.moveTo(annotation.x2, annotation.y2);
    context.lineTo(
      annotation.x2 - head * Math.cos(angle + Math.PI / 7),
      annotation.y2 - head * Math.sin(angle + Math.PI / 7),
    );
    context.stroke();
  }
  context.restore();
}

export function ImageAnnotator({ path, onSaved }: Props) {
  const { t } = useI18n();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const imageRef = useRef<HTMLImageElement | null>(null);
  const [tool, setTool] = useState<Tool>("rectangle");
  const [annotations, setAnnotations] = useState<Annotation[]>([]);
  const [cropRect, setCropRect] = useState<Annotation | null>(null);
  const [preview, setPreview] = useState<Annotation | null>(null);
  const [start, setStart] = useState<Point | null>(null);
  const [busy, setBusy] = useState(false);
  const [copying, setCopying] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [stamp, setStamp] = useState<Stamp>("check");
  const [textPoint, setTextPoint] = useState<Point | null>(null);
  const [textDraft, setTextDraft] = useState("");
  const [imageHistory, setImageHistory] = useState<ImageSnapshot[]>([]);

  useEffect(() => {
    let cancelled = false;
    setAnnotations([]);
    setCropRect(null);
    setPreview(null);
    setStart(null);
    setTextPoint(null);
    setTextDraft("");
    setImageHistory([]);
    const image = new Image();
    image.onload = () => {
      if (cancelled) return;
      imageRef.current = image;
      const canvas = canvasRef.current;
      if (!canvas) return;
      canvas.width = image.naturalWidth;
      canvas.height = image.naturalHeight;
      const context = canvas.getContext("2d");
      if (context) context.drawImage(image, 0, 0);
    };
    image.onerror = () => {
      if (!cancelled) setError(t("pinLoadFailed"));
    };
    if (isTauri()) {
      void api
        .readImageData(path)
        .then((url) => {
          if (!cancelled) image.src = url;
        })
        .catch((reason) => {
          if (!cancelled) setError(String(reason).replace(/^Error:\s*/, ""));
        });
    } else {
      image.src = "/demo-preview.svg";
    }
    return () => {
      cancelled = true;
    };
  }, [path]);

  useEffect(() => {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (!canvas || !image) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    context.clearRect(0, 0, canvas.width, canvas.height);
    context.drawImage(image, 0, 0);
    for (const annotation of annotations) {
      drawAnnotation(context, annotation, canvas.width);
    }
    if (cropRect) {
      drawAnnotation(context, cropRect, canvas.width);
      context.save();
      context.fillStyle = "rgba(20, 14, 12, 0.46)";
      const left = Math.min(cropRect.x1, cropRect.x2);
      const top = Math.min(cropRect.y1, cropRect.y2);
      const right = Math.max(cropRect.x1, cropRect.x2);
      const bottom = Math.max(cropRect.y1, cropRect.y2);
      context.fillRect(0, 0, canvas.width, top);
      context.fillRect(0, bottom, canvas.width, canvas.height - bottom);
      context.fillRect(0, top, left, bottom - top);
      context.fillRect(right, top, canvas.width - right, bottom - top);
      context.restore();
    }
    if (preview) drawAnnotation(context, preview, canvas.width);
  }, [annotations, cropRect, preview]);

  function pointFromEvent(event: React.PointerEvent<HTMLCanvasElement>): Point {
    const canvas = canvasRef.current!;
    const rect = canvas.getBoundingClientRect();
    return {
      x: ((event.clientX - rect.left) / rect.width) * canvas.width,
      y: ((event.clientY - rect.top) / rect.height) * canvas.height,
    };
  }

  function renderEditedCanvas(): HTMLCanvasElement | null {
    const canvas = canvasRef.current;
    const image = imageRef.current;
    if (!canvas || !image) return null;
    const output = document.createElement("canvas");
    output.width = canvas.width;
    output.height = canvas.height;
    const context = output.getContext("2d");
    if (!context) return null;
    context.drawImage(image, 0, 0);
    for (const annotation of annotations) {
      drawAnnotation(context, annotation, output.width);
    }
    return output;
  }

  async function saveCopy(copyAfterSave: boolean) {
    if (busy || copying) return;
    setCopying(copyAfterSave);
    setBusy(true);
    setError(null);
    try {
      const canvas = renderEditedCanvas();
      if (!canvas) return;
      const saved = await api.saveImageData(path, canvas.toDataURL("image/png"), " (annotated)");
      onSaved(saved);
      if (copyAfterSave) {
        await writeImage(saved);
      }
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusy(false);
      setCopying(false);
    }
  }

  function applyCrop() {
    if (!cropRect) return;
    const canvas = renderEditedCanvas();
    if (!canvas) return;
    const left = Math.max(0, Math.min(cropRect.x1, cropRect.x2));
    const top = Math.max(0, Math.min(cropRect.y1, cropRect.y2));
    const right = Math.min(canvas.width, Math.max(cropRect.x1, cropRect.x2));
    const bottom = Math.min(canvas.height, Math.max(cropRect.y1, cropRect.y2));
    const width = Math.round(right - left);
    const height = Math.round(bottom - top);
    if (width < 4 || height < 4) return;
    pushImageHistory();
    const output = document.createElement("canvas");
    output.width = width;
    output.height = height;
    output.getContext("2d")?.drawImage(canvas, left, top, width, height, 0, 0, width, height);
    const next = new Image();
    next.onload = () => {
      imageRef.current = next;
      const target = canvasRef.current;
      if (target) {
        target.width = width;
        target.height = height;
      }
      setAnnotations([]);
      setPreview(null);
      setCropRect(null);
      setStart(null);
    };
    next.src = output.toDataURL("image/png");
  }

  function addBackground() {
    const canvas = renderEditedCanvas();
    if (!canvas) return;
    pushImageHistory();
    const padding = Math.max(24, Math.round(Math.max(canvas.width, canvas.height) * 0.07));
    const output = document.createElement("canvas");
    output.width = canvas.width + padding * 2;
    output.height = canvas.height + padding * 2;
    const context = output.getContext("2d");
    if (!context) return;
    const gradient = context.createLinearGradient(0, 0, output.width, output.height);
    gradient.addColorStop(0, "#ffb45f");
    gradient.addColorStop(0.52, "#ff7048");
    gradient.addColorStop(1, "#c56fe6");
    context.fillStyle = gradient;
    context.fillRect(0, 0, output.width, output.height);
    const radius = Math.max(14, Math.round(padding * 0.45));
    context.save();
    context.beginPath();
    context.roundRect(padding, padding, canvas.width, canvas.height, radius);
    context.clip();
    context.drawImage(canvas, padding, padding);
    context.restore();
    const next = new Image();
    next.onload = () => {
      imageRef.current = next;
      const target = canvasRef.current;
      if (target) {
        target.width = output.width;
        target.height = output.height;
      }
      setAnnotations([]);
      setPreview(null);
      setCropRect(null);
      setStart(null);
    };
    next.src = output.toDataURL("image/png");
  }

  function removeBackground() {
    const canvas = renderEditedCanvas();
    if (!canvas) return;
    const context = canvas.getContext("2d");
    if (!context) return;
    pushImageHistory();
    const imageData = context.getImageData(0, 0, canvas.width, canvas.height);
    const data = imageData.data;
    const samples = [
      [0, 0],
      [canvas.width - 1, 0],
      [0, canvas.height - 1],
      [canvas.width - 1, canvas.height - 1],
    ].map(([x, y]) => {
      const index = (y * canvas.width + x) * 4;
      return [data[index], data[index + 1], data[index + 2]];
    });
    const background = samples.reduce(
      (sum, color) => [
        sum[0] + color[0] / samples.length,
        sum[1] + color[1] / samples.length,
        sum[2] + color[2] / samples.length,
      ],
      [0, 0, 0],
    );
    const tolerance = 42;
    for (let index = 0; index < data.length; index += 4) {
      const distance = Math.hypot(
        data[index] - background[0],
        data[index + 1] - background[1],
        data[index + 2] - background[2],
      );
      if (distance <= tolerance) {
        data[index + 3] = 0;
      }
    }
    context.putImageData(imageData, 0, 0);
    const next = new Image();
    next.onload = () => {
      imageRef.current = next;
      setAnnotations([]);
      setPreview(null);
      setCropRect(null);
      setStart(null);
    };
    next.src = canvas.toDataURL("image/png");
  }

  function pushImageHistory() {
    const image = imageRef.current;
    if (!image?.src) return;
    setImageHistory((current) => [
      ...current,
      { src: image.src, annotations: [...annotations] },
    ]);
  }

  function undo() {
    if (annotations.length > 0) {
      setAnnotations((current) => current.slice(0, -1));
      return;
    }
    const previous = imageHistory[imageHistory.length - 1];
    if (!previous) return;
    setImageHistory((current) => current.slice(0, -1));
    const next = new Image();
    next.onload = () => {
      imageRef.current = next;
      const target = canvasRef.current;
      if (target) {
        target.width = next.naturalWidth;
        target.height = next.naturalHeight;
      }
      setAnnotations(previous.annotations);
      setPreview(null);
      setCropRect(null);
      setStart(null);
    };
    next.src = previous.src;
  }

  const tools = [
    { id: "rectangle" as const, icon: Square, label: t("rectangle") },
    { id: "arrow" as const, icon: ArrowUpRight, label: t("arrow") },
    { id: "highlight" as const, icon: Highlighter, label: t("highlight") },
    { id: "redact" as const, icon: EyeOff, label: t("redact") },
    { id: "pixelate" as const, icon: Grid3X3, label: t("pixelate") },
    { id: "blur" as const, icon: Droplets, label: t("blur") },
    { id: "number" as const, icon: ListOrdered, label: t("number") },
    { id: "stamp" as const, icon: CircleCheck, label: t("stamp") },
    { id: "crop" as const, icon: Crop, label: t("crop") },
    { id: "text" as const, icon: Type, label: t("textAnnotation") },
  ];

  const stamps: { id: Stamp; icon: typeof CircleCheck; label: string }[] = [
    { id: "check", icon: CircleCheck, label: t("stampCheck") },
    { id: "cross", icon: CircleX, label: t("stampCross") },
    { id: "star", icon: Star, label: t("stampStar") },
  ];

  return (
    <div className="annotator-shell">
      <div className="annotator-toolbar">
        {tools.map(({ id, icon: Icon, label }) => (
          <button
            key={id}
            className={`annotator-tool ${tool === id ? "is-active" : ""}`}
            title={label}
            onClick={() => setTool(id)}
          >
            <Icon size={15} />
            <span>{label}</span>
          </button>
        ))}
        <span className="annotator-separator" />
        {tool === "stamp"
          ? stamps.map(({ id, icon: Icon, label }) => (
              <button
                key={id}
                className={`annotator-tool annotator-stamp ${stamp === id ? "is-active" : ""}`}
                title={label}
                onClick={() => setStamp(id)}
              >
                <Icon size={15} />
              </button>
            ))
          : null}
        <span className="annotator-separator" />
        {cropRect ? (
          <button className="annotator-tool is-primary" onClick={applyCrop}>
            <Check size={15} />
            {t("applyCrop")}
          </button>
        ) : null}
        <button className="annotator-tool" onClick={addBackground} title={t("addBackground")}>
          <ImagePlus size={15} />
          {t("addBackground")}
        </button>
        <button className="annotator-tool" onClick={removeBackground} title={t("cutout")}>
          <Scissors size={15} />
          {t("cutout")}
        </button>
        <button
          className="annotator-tool"
          title={t("undo")}
          disabled={annotations.length === 0 && imageHistory.length === 0}
          onClick={undo}
        >
          <Undo2 size={15} />
        </button>
        <button className="annotator-tool is-primary" onClick={() => void saveCopy(false)}>
          {busy && !copying ? <LoaderCircle size={15} className="spin" /> : <Save size={15} />}
          {t("saveCopy")}
        </button>
        <button className="annotator-tool" onClick={() => void saveCopy(true)}>
          {copying ? <LoaderCircle size={15} className="spin" /> : <Copy size={15} />}
          {t("copy")}
        </button>
      </div>
      <div className="annotator-canvas-wrap">
        <canvas
          ref={canvasRef}
          onPointerDown={(event) => {
            const point = pointFromEvent(event);
            if (tool === "text") {
              setTextPoint(point);
              setTextDraft("");
              return;
            }
            if (tool === "stamp") {
              setAnnotations((current) => [
                ...current,
                { tool: "stamp", stamp, x1: point.x, y1: point.y, x2: point.x, y2: point.y },
              ]);
              return;
            }
            if (tool === "number") {
              setAnnotations((current) => {
                const nextNumber =
                  current.reduce(
                    (max, annotation) =>
                      annotation.tool === "number"
                        ? Math.max(max, annotation.number ?? 0)
                        : max,
                    0,
                  ) + 1;
                return [
                  ...current,
                  {
                    tool: "number" as const,
                    x1: point.x,
                    y1: point.y,
                    x2: point.x,
                    y2: point.y,
                    number: nextNumber,
                  },
                ];
              });
              return;
            }
            setStart(point);
            setPreview({ tool, x1: point.x, y1: point.y, x2: point.x, y2: point.y });
            event.currentTarget.setPointerCapture(event.pointerId);
          }}
          onPointerMove={(event) => {
            if (!start) return;
            const point = pointFromEvent(event);
            setPreview({ tool, x1: start.x, y1: start.y, x2: point.x, y2: point.y });
          }}
          onPointerUp={(event) => {
            if (!start) return;
            const point = pointFromEvent(event);
            const annotation = { tool, x1: start.x, y1: start.y, x2: point.x, y2: point.y };
            if (Math.hypot(point.x - start.x, point.y - start.y) > 3) {
              if (tool === "crop") {
                setCropRect(annotation);
              } else {
                setAnnotations((current) => [...current, annotation]);
              }
            }
            setStart(null);
            setPreview(null);
            event.currentTarget.releasePointerCapture(event.pointerId);
          }}
        />
        {error ? <div className="annotator-error">{error}</div> : null}
        {textPoint ? (
          <div className="annotator-text-entry">
            <input
              autoFocus
              value={textDraft}
              placeholder={t("textAnnotation")}
              onChange={(event) => setTextDraft(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Escape") {
                  setTextPoint(null);
                  setTextDraft("");
                }
                if (event.key === "Enter" && textDraft.trim()) {
                  setAnnotations((current) => [
                    ...current,
                    {
                      tool: "text",
                      x1: textPoint.x,
                      y1: textPoint.y,
                      x2: textPoint.x,
                      y2: textPoint.y,
                      text: textDraft.trim(),
                    },
                  ]);
                  setTextPoint(null);
                  setTextDraft("");
                }
              }}
            />
            <button
              onClick={() => {
                if (!textDraft.trim()) return;
                setAnnotations((current) => [
                  ...current,
                  {
                    tool: "text",
                    x1: textPoint.x,
                    y1: textPoint.y,
                    x2: textPoint.x,
                    y2: textPoint.y,
                    text: textDraft.trim(),
                  },
                ]);
                setTextPoint(null);
                setTextDraft("");
              }}
            >
              <Check size={14} />
            </button>
          </div>
        ) : null}
      </div>
    </div>
  );
}
