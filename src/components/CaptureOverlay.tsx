import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, Gauge, LoaderCircle, Mic, MicOff, ScanLine, Square, X } from "lucide-react";
import { useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { messageFrom, normalizeSelection } from "../lib/utils";
import { FEEDBACK } from "../lib/feedback";
import type { CaptureSession, RecordingCapabilities } from "../types";

interface Point {
  x: number;
  y: number;
}

/** Choices the frame-rate chip cycles through, low to high. */
const FRAME_RATES = [15, 24, 30];

export function CaptureOverlay() {
  const { t } = useI18n();
  const [session, setSession] = useState<CaptureSession | null>(null);
  const [start, setStart] = useState<Point | null>(null);
  const [end, setEnd] = useState<Point | null>(null);
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState(false);
  const [status, setStatus] = useState<string | null>(null);
  const [audio, setAudio] = useState(false);
  // Which recording switches this platform actually implements. Offering a control the
  // backend ignores would promise something the file cannot deliver, so the toolbar
  // only renders what the platform reports.
  const [capabilities, setCapabilities] = useState<RecordingCapabilities>({
    audio: true,
    fps: false,
  });
  // Frame rate is only sent where the capture backend accepts one, so this is a real
  // request rather than a number that gets dropped on the floor.
  const [fps, setFps] = useState(30);
  const stageRef = useRef<HTMLDivElement>(null);
  const toolbarRef = useRef<HTMLDivElement>(null);
  const [toolbarWidth, setToolbarWidth] = useState(260);

  useEffect(() => {
    if (!isTauri()) {
      setSession({
        mode: "screenshot",
        source: {
          path: "/demo-preview.svg",
          width: 1200,
          height: 800,
          monitorX: 0,
          monitorY: 0,
          scaleFactor: 1,
        },
      });
      return;
    }
    void api.getCaptureSession().then((next) => {
      if (next) setSession(next);
    });
    void api.recordingCapabilities().then(setCapabilities).catch(() => undefined);
  }, []);

  useEffect(() => {
    if (!isTauri()) return;
    let unlisten: (() => void) | undefined;
    void listen<CaptureSession>("capture://ready", (event) => {
      setSession(event.payload);
      setStart(null);
      setEnd(null);
      setDragging(false);
      setBusy(false);
      setStatus(null);
      setAudio(false);
    }).then((stop) => {
      unlisten = stop;
    });
    return () => unlisten?.();
  }, []);

  const selection = useMemo(() => {
    if (!start || !end || !stageRef.current) return null;
    const bounds = stageRef.current.getBoundingClientRect();
    return normalizeSelection(start.x, start.y, end.x, end.y, bounds.width, bounds.height);
  }, [start, end]);

  async function cancel() {
    if (busy) return;
    if (!isTauri()) {
      setStatus(t("cancel"));
      return;
    }
    await api.cancelCapture().catch(() => undefined);
  }

  async function confirm() {
    if (busy || !selection || selection.width < 8 || selection.height < 8) {
      setStatus(t("dragSelect"));
      window.setTimeout(() => setStatus(null), FEEDBACK.hint);
      return;
    }
    if (!isTauri()) {
      setBusy(true);
      setStatus(`${t("confirm")} ${Math.round(selection.width)} × ${Math.round(selection.height)}`);
      window.setTimeout(() => {
        setBusy(false);
        setStatus(null);
      }, 1200);
      return;
    }
    setBusy(true);
    setStatus(t("processing"));
    try {
      if (session?.mode === "recording") {
        // Only the switches this platform honours are sent, so nothing in the
        // toolbar can promise a recording option the backend would drop.
        await api.startRecording(selection.normalized, {
          audio: capabilities.audio ? audio : false,
          ...(capabilities.fps ? { fps } : {}),
        });
      } else {
        await api.captureCrop(selection.normalized);
      }
    } catch (error) {
      setBusy(false);
      setStatus(t(messageFrom(error)));
      window.setTimeout(() => setStatus(null), FEEDBACK.error);
    }
  }

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape") void cancel();
      if (event.key === "Enter") void confirm();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  });

  useLayoutEffect(() => {
    const node = toolbarRef.current;
    if (!node) return;
    // Keep the measured width so the clamping below centres the toolbar wherever the
    // selection sits, without hard-coding how many chips the platform shows.
    setToolbarWidth((current) => (current === node.offsetWidth ? current : node.offsetWidth));
  });

  if (!session) {
    return <div className="capture-loading" />;
  }

  const imageUrl = isTauri() ? convertFileSrc(session.source.path) : session.source.path;
  const selectionStyle = selection
    ? {
        left: selection.left,
        top: selection.top,
        width: selection.width,
        height: selection.height,
      }
    : undefined;

  const toolbarTop = selection
    ? Math.min(
        Math.max(12, selection.top + selection.height + 14),
        Math.max(12, window.innerHeight - 68),
      )
    : 0;
  const toolbarLeft = selection
    ? Math.min(
        Math.max(12, selection.left + selection.width / 2 - toolbarWidth / 2),
        Math.max(12, window.innerWidth - toolbarWidth - 12),
      )
    : 0;

  return (
    <div
      ref={stageRef}
      className={`capture-stage ${dragging ? "is-dragging" : ""}`}
      onPointerDown={(event) => {
        if (busy || event.button !== 0 || event.target !== event.currentTarget) return;
        const point = { x: event.clientX, y: event.clientY };
        setStart(point);
        setEnd(point);
        setDragging(true);
        event.currentTarget.setPointerCapture(event.pointerId);
      }}
      onPointerMove={(event) => {
        if (!dragging) return;
        setEnd({ x: event.clientX, y: event.clientY });
      }}
      onPointerUp={(event) => {
        if (!dragging) return;
        setEnd({ x: event.clientX, y: event.clientY });
        setDragging(false);
        if (event.currentTarget.hasPointerCapture(event.pointerId)) {
          event.currentTarget.releasePointerCapture(event.pointerId);
        }
      }}
      onPointerCancel={() => setDragging(false)}
    >
      <img
        className="capture-screen"
        src={imageUrl}
        alt=""
        draggable={false}
        onDragStart={(event) => event.preventDefault()}
      />
      <div className="capture-dim" />
      {selection ? (
        <>
          <div className="capture-selection" style={selectionStyle}>
            <span className="capture-corner capture-corner-tl" />
            <span className="capture-corner capture-corner-tr" />
            <span className="capture-corner capture-corner-bl" />
            <span className="capture-corner capture-corner-br" />
            <div className="capture-size-chip">
              {Math.max(0, Math.round(selection.width))} × {Math.max(0, Math.round(selection.height))}
            </div>
          </div>
          <div
            ref={toolbarRef}
            className="capture-toolbar"
            style={{ top: toolbarTop, left: toolbarLeft }}
            onPointerDown={(event) => event.stopPropagation()}
            onClick={(event) => event.stopPropagation()}
          >
            {session.mode === "recording" ? (
              <>
                {capabilities.audio ? (
                  <button
                    className={`capture-tool ${audio ? "is-selected" : ""}`}
                    onClick={() => setAudio((value) => !value)}
                    title={t("audio")}
                  >
                    {audio ? <Mic size={17} /> : <MicOff size={17} />}
                  </button>
                ) : null}
                {capabilities.fps ? (
                  <button
                    className="capture-tool capture-fps"
                    onClick={() =>
                      setFps(
                        FRAME_RATES[
                          (FRAME_RATES.indexOf(fps) + 1) % FRAME_RATES.length
                        ] ?? 30,
                      )
                    }
                    title={t("frameRate")}
                  >
                    <Gauge size={15} />
                    {fps}
                  </button>
                ) : null}
                <button className="capture-tool capture-record" onClick={() => void confirm()}>
                  <Square size={14} fill="currentColor" />
                  {t("startRecording")}
                </button>
              </>
            ) : (
              <button className="capture-tool capture-confirm" onClick={() => void confirm()}>
                <Check size={18} strokeWidth={2.4} />
                {t("confirm")}
              </button>
            )}
            <button className="capture-tool" onClick={() => void cancel()} title={t("cancel")}>
              <X size={17} />
            </button>
          </div>
        </>
      ) : (
        <div className="capture-start-hint">
          <span className="capture-icon">
            {session.mode === "recording" ? <ScanLine size={22} /> : <ScanLine size={22} />}
          </span>
          <div>
            <strong>{t("dragSelect")}</strong>
          </div>
          <kbd>ESC</kbd>
        </div>
      )}
      {busy ? (
        <div className="capture-processing" role="status" aria-live="polite">
          <LoaderCircle size={20} className="spin" />
          {status || t("processing")}
        </div>
      ) : status ? (
        <div className="capture-status" role="status" aria-live="polite">{status}</div>
      ) : null}
    </div>
  );
}
