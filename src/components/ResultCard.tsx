import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { emitTo } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { writeImage, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { Copy, FileVideo, FolderSearch, Images, Send } from "lucide-react";
import { useCallback, useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { basename, decodeHexParam } from "../lib/utils";
import type { CaptureResult } from "../types";

const THUMB_EXTENSIONS = ["png", "jpg", "jpeg", "webp", "gif", "bmp", "tiff", "tif"];

function readInfo(): CaptureResult | null {
  const raw = decodeHexParam(new URLSearchParams(window.location.search).get("info"));
  if (!raw) return null;
  try {
    const parsed = JSON.parse(raw) as CaptureResult;
    return parsed.path ? parsed : null;
  } catch {
    return null;
  }
}

/**
 * The card that appears the moment a screenshot or recording is saved. It answers
 * the four things a capture raises, in the order they get asked: put it on the
 * clipboard, keep it on screen, hand it to another machine, open it. It hides
 * itself, so ignoring it is a valid answer too.
 */
export function ResultCard() {
  const { t } = useI18n();
  const info = readInfo();
  const ttl = info?.ttlMs ?? 8000;
  const [remaining, setRemaining] = useState(ttl);
  const [failed, setFailed] = useState(false);
  const [note, setNote] = useState<string | null>(null);
  const hold = useRef<number | null>(null);

  const close = useCallback(() => {
    void getCurrentWindow()
      .close()
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    const started = Date.now();
    const tick = window.setInterval(() => {
      const left = Math.max(0, ttl - (Date.now() - started));
      setRemaining(left);
      if (left === 0) {
        window.clearInterval(tick);
        close();
      }
    }, 100);
    return () => window.clearInterval(tick);
  }, [close, ttl]);

  // Hovering pauses the fuse: the difference between a helper and a reflex test.
  function pause() {
    if (hold.current) window.clearTimeout(hold.current);
  }

  function resume() {
    pause();
    hold.current = window.setTimeout(close, 2500);
  }

  useEffect(() => () => pause(), []);

  async function copy() {
    if (!info) return;
    try {
      if (info.kind === "image") await writeImage(info.path);
      else await writeText(info.path);
      setNote(t("copied"));
    } catch (error) {
      setNote(String(error));
    }
  }

  async function open() {
    if (!info) return;
    await api.openInEditor(info.path);
    close();
  }

  async function pin() {
    if (!info || info.kind !== "image") return;
    await api.pinImage(info.path);
    close();
  }

  async function reveal() {
    if (!info) return;
    await api.revealFile(info.path);
    close();
  }

  function send() {
    if (!info) return;
    // The main window owns the peer list, so the request goes there.
    void emitTo("main", "app://send-files", { paths: [info.path] });
    close();
  }

  if (!info) return null;
  const extension = info.path.split(".").pop()?.toLowerCase() ?? "";
  const showThumb = info.kind === "image" && THUMB_EXTENSIONS.includes(extension) && !failed;
  const percent = ttl > 0 ? (remaining / ttl) * 100 : 0;

  return (
    <div
      className="result-card"
      onMouseEnter={pause}
      onMouseLeave={resume}
      role="status"
      aria-label={t("captureSaved")}
    >
      <div className={`result-card-thumb ${info.kind}`}>
        {showThumb ? (
          <img
            src={isTauri() ? convertFileSrc(info.path) : info.path}
            alt=""
            onError={() => setFailed(true)}
          />
        ) : info.kind === "video" ? (
          <FileVideo size={18} />
        ) : (
          <Images size={18} />
        )}
      </div>
      <div className="result-card-copy" data-tauri-drag-region>
        <strong>{t("captureSaved")}</strong>
        <span title={info.path}>{basename(info.path)}</span>
        {note ? <em>{note}</em> : null}
      </div>
      <div className="result-card-actions">
        <button title={t("copy")} onClick={() => void copy()}>
          <Copy size={14} />
        </button>
        {info.kind === "image" ? (
          <button title={t("pin")} onClick={() => void pin()}>
            <Images size={14} />
          </button>
        ) : null}
        <button title={t("sendTo")} onClick={() => void send()}>
          <Send size={14} />
        </button>
        <button title={t("revealFile")} onClick={() => void reveal()}>
          <FolderSearch size={14} />
        </button>
        <button className="result-card-open" onClick={() => void open()}>
          {t("edit")}
        </button>
      </div>
      <div className="result-card-fuse" style={{ width: `${percent}%` }} />
    </div>
  );
}
