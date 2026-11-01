import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { writeImage, writeText } from "@tauri-apps/plugin-clipboard-manager";
import { save } from "@tauri-apps/plugin-dialog";
import {
  Check,
  CircleAlert,
  Clipboard,
  Copy,
  ExternalLink,
  FileText,
  FolderOpen,
  LoaderCircle,
  Pin,
  ScanText,
  X,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import { recognizeImage } from "../lib/ocr";
import { FEEDBACK } from "../lib/feedback";
import { basename, dirname, extensionOf, IS_MAC, messageFrom } from "../lib/utils";
import { history } from "../lib/history";
import type { EditorInfo } from "../types";
import { ImageAnnotator } from "./ImageAnnotator";

export function Editor() {
  const { t } = useI18n();
  const [session, setSession] = useState<EditorInfo | null>(null);
  const [ocrOpen, setOcrOpen] = useState(false);
  const [ocrText, setOcrText] = useState("");
  const [ocrProgress, setOcrProgress] = useState(0);
  const [ocrStatus, setOcrStatus] = useState("");
  const [ocrRunning, setOcrRunning] = useState(false);
  const [ocrFailed, setOcrFailed] = useState(false);
  const [pinBusy, setPinBusy] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);
  const [copiedIsError, setCopiedIsError] = useState(false);
  const [textContent, setTextContent] = useState("");
  const copyTimer = useRef<number | null>(null);
  const sessionRef = useRef<EditorInfo | null>(null);

  function applySession(next: EditorInfo | null) {
    if (!next) return;
    sessionRef.current = next;
    setSession(next);
    setOcrOpen(false);
    setOcrText("");
    setOcrProgress(0);
    setOcrStatus("");
    setTextContent("");
    // Anything opened for editing belongs in the library. Rust assigns the kind
    // and timestamp, so the editor does not classify the file a second time.
    void history.record([next.path]).catch(() => undefined);
  }

  useEffect(() => {
    let disposed = false;
    const unlisteners: Array<() => void> = [];
    if (!isTauri()) {
      applySession({ path: "/demo-preview.svg", kind: "image" });
      return;
    }
    void api.getEditorSession().then((next) => {
      if (!disposed) applySession(next);
    });
    void listen<EditorInfo>("editor://open", (event) => applySession(event.payload)).then(
      (unlisten) => unlisteners.push(unlisten),
    );
    void listen("editor://run-ocr", () => void runOcr()).then((unlisten) =>
      unlisteners.push(unlisten),
    );
    return () => {
      disposed = true;
      unlisteners.forEach((unlisten) => unlisten());
      if (copyTimer.current) window.clearTimeout(copyTimer.current);
    };
  }, []);

  useEffect(() => {
    if (session?.kind !== "text") return;
    let cancelled = false;
    void api.readTextFile(session.path).then((text) => {
      if (!cancelled) setTextContent(text);
    });
    return () => {
      cancelled = true;
    };
  }, [session?.kind, session?.path]);

  /** The window's one transient notice. A failure must not arrive wearing a tick. */
  function flashNotice(label: string, failed = false) {
    if (copyTimer.current) window.clearTimeout(copyTimer.current);
    setCopied(label);
    setCopiedIsError(failed);
    copyTimer.current = window.setTimeout(() => setCopied(null), FEEDBACK.done);
  }

  async function copy() {
    if (!session) return;
    // A clipboard the OS refuses is a result the user has to be told about; it
    // used to fail in silence here while every other window reported it.
    try {
      if (session.kind === "image") await writeImage(session.path);
      else if (session.kind === "text") await writeText(textContent);
      else await writeText(session.path);
      flashNotice(t("copy"));
    } catch (error) {
      flashNotice(t(messageFrom(error)), true);
    }
  }

  async function saveAs() {
    if (!session) return;
    const ext = extensionOf(session.path);
    const target = await save({
      defaultPath: `${dirname(session.path)}/${basename(session.path)}`,
      filters: ext ? [{ name: ext.toUpperCase(), extensions: [ext] }] : undefined,
    });
    if (!target) return;
    try {
      await api.copyFileTo(session.path, target);
      flashNotice(t("saved"));
    } catch (error) {
      flashNotice(t(messageFrom(error)), true);
    }
  }

  async function runOcr() {
    const current = sessionRef.current;
    if (!current || ocrRunning) return;
    setOcrOpen(true);
    setOcrRunning(true);
    setOcrText("");
    setOcrProgress(0);
      setOcrFailed(false);
    setOcrStatus(t("ocrRunning"));
    try {
      const source = isTauri() ? convertFileSrc(current.path) : current.path;
      const result = await recognizeImage(source, (progress, status) => {
        setOcrProgress(progress);
        setOcrStatus(status || t("ocrRunning"));
      });
      setOcrText(result.text || t("ocrEmpty"));
      if (result.text) {
        await writeText(result.text);
        flashNotice(t("copyText"));
      }
      setOcrProgress(1);
      setOcrStatus("");
    } catch (error) {
      // Nothing was read, so the panel says only that and stays out of the way;
      // the reason belongs on the notice, which is where this window reports.
      setOcrFailed(true);
      flashNotice(t(messageFrom(error)), true);
      setOcrStatus("");
    } finally {
      setOcrRunning(false);
    }
  }

  async function pinToScreen() {
    const current = sessionRef.current;
    if (!current || pinBusy) return;
    setPinBusy(true);
    try {
      await api.pinImage(current.path);
    } catch (error) {
      flashNotice(t(messageFrom(error)), true);
    } finally {
      setPinBusy(false);
    }
  }

  if (!session) {
    return <div className="editor-loading" />;
  }

  const url = isTauri() ? convertFileSrc(session.path) : session.path;
  const name = basename(session.path);

  return (
    <div className={`editor-shell ${IS_MAC ? "is-mac" : ""}`}>
      <header className="editor-titlebar" data-tauri-drag-region>
        <div className="home-brand" data-tauri-drag-region>
          <img className="home-mark" src="/shift-icon.png" alt="" aria-hidden="true" />
          <strong>Shift</strong>
          <span className="home-brand-sub">Your Files</span>
        </div>
        <div className="editor-title" data-tauri-drag-region>
          {name}
        </div>
        <button
          className="home-icon-button editor-close"
          onClick={() => void api.closeEditor()}
          aria-label={t("close")}
          title={t("close")}
        >
          <X size={16} />
        </button>
      </header>

      <div className="editor-toolbar">
        <div className="editor-tool-group">
          <button className="editor-button" onClick={() => void copy()}>
            <Copy size={16} />
            {t("copy")}
          </button>
          <button className="editor-button" onClick={() => void saveAs()}>
            <Clipboard size={16} />
            {t("saveAs")}
          </button>
          {session.kind === "image" ? (
            <button className={`editor-button ${ocrOpen ? "is-active" : ""}`} onClick={() => void runOcr()}>
              <ScanText size={16} />
              {t("ocr")}
            </button>
          ) : null}
          {session.kind === "image" ? (
            <button
              className="editor-button"
              onClick={() => void pinToScreen()}
              disabled={pinBusy}
            >
              {pinBusy ? <LoaderCircle size={16} className="spin" /> : <Pin size={16} />}
              {t("pin")}
            </button>
          ) : null}
        </div>
        <div className="editor-tool-group">
          <span className="editor-path" title={session.path}>
            {dirname(session.path)}
          </span>
          <button className="editor-icon" title={t("open")} onClick={() => void api.openFile(session.path)}>
            <ExternalLink size={16} />
          </button>
          <button className="editor-icon" title={t("revealFile")} onClick={() => void api.revealFile(session.path)}>
            <FolderOpen size={16} />
          </button>
        </div>
      </div>

      <main className={`editor-main ${ocrOpen ? "has-ocr" : ""}`}>
        <section className="editor-canvas">
          {session.kind === "image" ? (
            <ImageAnnotator
              path={session.path}
              onSaved={(nextPath) => {
                applySession({ path: nextPath, kind: "image" });
                flashNotice(t("saved"));
              }}
            />
          ) : session.kind === "video" ? (
            <video src={url} controls autoPlay />
          ) : session.kind === "text" ? (
            <pre className="editor-text-preview">{textContent}</pre>
          ) : (
            <div className="editor-file-fallback">
              <FileText size={54} strokeWidth={1.4} />
              <strong>{name}</strong>
              <button onClick={() => void api.openFile(session.path)}>{t("open")}</button>
            </div>
          )}
        </section>

        {ocrOpen ? (
          <aside className="ocr-panel">
            <div className="ocr-panel-head">
              <div>
                <span className="ocr-eyebrow">{t("ocr")}</span>
                <h2>{t("ocrTitle")}</h2>
              </div>
              <button
                className="editor-icon"
                onClick={() => setOcrOpen(false)}
                aria-label={t("close")}
                title={t("close")}
              >
                <X size={16} />
              </button>
            </div>
            {ocrRunning ? (
              <div className="ocr-progress">
                <div className="ocr-progress-row">
                  <LoaderCircle size={17} className="spin" />
                  <span>{ocrStatus}</span>
                  <b>{Math.round(ocrProgress * 100)}%</b>
                </div>
                <div className="ocr-progress-bar">
                  <span style={{ width: `${Math.max(4, ocrProgress * 100)}%` }} />
                </div>
              </div>
            ) : null}
            <textarea
              className="ocr-output"
              value={ocrText}
              onChange={(event) => setOcrText(event.target.value)}
              placeholder={t(ocrFailed ? "ocrNothing" : "ocrEmpty")}
              spellCheck={false}
            />
            <div className="ocr-actions">
              <button className="editor-primary" onClick={() => void writeText(ocrText).then(() => flashNotice(t("copyText")))}>
                <Copy size={15} />
                {t("copyText")}
              </button>
              <button className="editor-secondary" onClick={() => void runOcr()}>
                <ScanText size={15} />
                {t("extractText")}
              </button>
            </div>
          </aside>
        ) : null}
      </main>

      {copied ? (
        <div
          className={`editor-toast ${copiedIsError ? "is-error" : ""}`}
          role={copiedIsError ? "alert" : "status"}
          aria-live="polite"
        >
          {copiedIsError ? <CircleAlert size={15} /> : <Check size={15} />}
          {copied}
        </div>
      ) : null}
    </div>
  );
}
