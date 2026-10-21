import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import {
  ArrowUpRight,
  Check,
  Circle,
  Clock3,
  FileAudio,
  File as FileGeneric,
  FileImage,
  FileSpreadsheet,
  FileText,
  FileType,
  FileVideo,
  FolderCog,
  FolderOpen,
  FolderSearch,
  FileOutput,
  Keyboard,
  LoaderCircle,
  Maximize,
  Pin,
  Presentation,
  RefreshCw,
  ScanLine,
  ScanText,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Wrench,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type CSSProperties } from "react";
import { useI18n } from "../i18n";
import { api } from "../lib/api";
import {
  basename,
  dirname,
  extensionOf,
  getRecentItems,
  IS_MAC,
  recentKind,
  rememberRecent,
  type RecentItem,
  setRecentItems,
  subscribeRecent,
  timeAgo,
} from "../lib/utils";
import type { AppSettings, HotkeyStatus, WriteCheck } from "../types";

const FALLBACK_SETTINGS: AppSettings = {
  fileOutputMode: "alongside",
  fileOutputDir: "~/Downloads/Shift",
  screenshotDir: "~/Pictures/Shift",
  recordingDir: "~/Movies/Shift",
  autoReveal: true,
  dragEnabled: true,
  autostart: false,
  defaultFileOutputDir: "~/Downloads/Shift",
  defaultScreenshotDir: "~/Pictures/Shift",
  defaultRecordingDir: "~/Movies/Shift",
};

const DROP_ICONS = [
  { icon: FileType, tone: "pdf" },
  { icon: FileText, tone: "document" },
  { icon: FileVideo, tone: "video" },
  { icon: FileImage, tone: "image" },
  { icon: FileAudio, tone: "audio" },
  { icon: FileSpreadsheet, tone: "sheet" },
  { icon: Presentation, tone: "presentation" },
];

type OutputKind = "files" | "screenshot" | "recording";

/** The three primary buttons double as drop targets: files dropped on a button run that action. */
type DropAction = "formats" | "tools" | "ocr";

const OCR_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "webp",
  "tiff",
  "tif",
  "bmp",
  "gif",
  "heic",
  "avif",
];

const ARC_STEP_DEG = 13;
const ARC_RADIUS = 140;
const ARC_LIFT = 32;

// WKWebView renders HEIC/AVIF natively; the icon fallback covers any decode failure.
const THUMB_EXTENSIONS = [
  "png",
  "jpg",
  "jpeg",
  "webp",
  "gif",
  "bmp",
  "tiff",
  "tif",
  "heic",
  "avif",
];

function RecentThumb({ item }: { item: RecentItem }) {
  const [failed, setFailed] = useState(false);
  const extension = extensionOf(item.path);
  const showImage = item.kind === "image" && THUMB_EXTENSIONS.includes(extension) && !failed;

  if (showImage) {
    return (
      <span className={`home-recent-icon ${item.kind}`}>
        <img
          src={isTauri() ? convertFileSrc(item.path) : item.path}
          alt=""
          loading="lazy"
          onError={() => setFailed(true)}
        />
      </span>
    );
  }
  return (
    <span className={`home-recent-icon ${item.kind}`}>
      {item.kind === "image" ? (
        <FileImage size={15} />
      ) : item.kind === "video" ? (
        <FileVideo size={15} />
      ) : item.kind === "text" ? (
        <FileText size={15} />
      ) : (
        <FileGeneric size={15} />
      )}
    </span>
  );
}

interface OutputLocationProps {
  label: string;
  path: string;
  onChoose: () => void;
  onReveal: () => void;
  onReset?: () => void;
  resetLabel: string;
  chooseLabel: string;
  revealLabel: string;
}

function OutputLocation({
  label,
  path,
  onChoose,
  onReveal,
  onReset,
  resetLabel,
  chooseLabel,
  revealLabel,
}: OutputLocationProps) {
  return (
    <div className="settings-output">
      <div className="settings-output-head">
        <span>{label}</span>
        {onReset ? (
          <button className="settings-link" onClick={onReset}>
            {resetLabel}
          </button>
        ) : null}
      </div>
      <div className="settings-path" title={path}>
        <FolderOpen size={14} />
        <span>{path}</span>
      </div>
      <div className="settings-output-actions">
        <button className="settings-small-button" onClick={onChoose}>
          <FolderSearch size={14} />
          {chooseLabel}
        </button>
        <button className="settings-small-button" onClick={onReveal}>
          {revealLabel}
        </button>
      </div>
    </div>
  );
}

export function MainWindow() {
  const { lang, setLang, t } = useI18n();
  const [dragging, setDragging] = useState(false);
  const [busy, setBusy] = useState<string | null>(null);
  const [toast, setToast] = useState<string | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [screenAccess, setScreenAccess] = useState<boolean | null>(null);
  const [checks, setChecks] = useState<WriteCheck[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [recent, setRecent] = useState<RecentItem[]>(getRecentItems);
  const toastTimer = useRef<number | null>(null);
  const [, setNowTick] = useState(0);
  const [dropAction, setDropAction] = useState<DropAction | null>(null);
  const [hotkeys, setHotkeys] = useState<HotkeyStatus | null>(null);
  const dropTargets = useRef<Partial<Record<DropAction, HTMLElement | null>>>({});

  function flash(message: string, duration = 2600) {
    if (toastTimer.current) window.clearTimeout(toastTimer.current);
    setToast(message);
    toastTimer.current = window.setTimeout(() => setToast(null), duration);
  }

  async function spring(action: string, task: () => Promise<void>) {
    if (busy) return;
    setBusy(action);
    try {
      await task();
    } catch (error) {
      flash(String(error), 3800);
    } finally {
      setBusy(null);
    }
  }

  async function chooseOcr() {
    const selected = await open({
      multiple: false,
      directory: false,
      filters: [
        {
          name: "Images",
          extensions: OCR_EXTENSIONS,
        },
      ],
    });
    if (!selected || Array.isArray(selected)) return;
    await api.openInEditor(selected);
    window.setTimeout(() => {
      void emitTo("editor", "editor://run-ocr", {});
    }, 450);
  }

  async function runOcrOn(paths: string[]) {
    const image = paths.find((path) => OCR_EXTENSIONS.includes(extensionOf(path)));
    if (!image) {
      // No image in the batch: the format ring is more useful than a failing OCR run.
      await api.showFilesInRadial(paths, "formats");
      return;
    }
    await api.openInEditor(image);
    window.setTimeout(() => {
      void emitTo("editor", "editor://run-ocr", {});
    }, 450);
  }

  // Drag positions arrive in physical pixels relative to the webview, while the
  // buttons are measured in CSS pixels, so both sides need the same scale.
  function actionAt(position: { x: number; y: number }): DropAction | null {
    const scale = window.devicePixelRatio || 1;
    const x = position.x / scale;
    const y = position.y / scale;
    const entries: Array<[DropAction, HTMLElement | null | undefined]> = [
      ["formats", dropTargets.current.formats],
      ["tools", dropTargets.current.tools],
      ["ocr", dropTargets.current.ocr],
    ];
    for (const [action, element] of entries) {
      if (!element) continue;
      const box = element.getBoundingClientRect();
      // Generous padding so a drop near a button still lands on it.
      if (x >= box.left - 10 && x <= box.right + 10 && y >= box.top - 12 && y <= box.bottom + 12) {
        return action;
      }
    }
    return null;
  }

  function dropFiles(paths: string[], action: DropAction | null) {
    if (paths.length === 0) return Promise.resolve();
    if (action === "ocr") return runOcrOn(paths);
    // Outside a button the ring keeps asking which action to run.
    return api.showFilesInRadial(paths, action ?? "actions").then(() => undefined);
  }

  async function chooseFiles(mode: "formats" | "tools") {
    const selected = await open({ multiple: true, directory: false });
    if (!selected) return;
    const files = Array.isArray(selected) ? selected : [selected];
    if (files.length === 0) return;
    await api.showFilesInRadial(files, mode);
  }

  async function applySettings(task: () => Promise<AppSettings>) {
    try {
      setSettings(await task());
    } catch (error) {
      flash(String(error), 3800);
    }
  }

  async function chooseFolder(kind: OutputKind) {
    if (!settings) return;
    const current =
      kind === "screenshot"
        ? settings.screenshotDir
        : kind === "recording"
          ? settings.recordingDir
          : settings.fileOutputDir;
    const selected = await open({ directory: true, multiple: false, defaultPath: current });
    if (!selected || Array.isArray(selected)) return;
    await applySettings(() => api.setOutputDirectory(kind, selected));
    setChecks(null);
  }

  async function resetFolder(kind: OutputKind) {
    await applySettings(() => api.setOutputDirectory(kind, null));
    setChecks(null);
  }

  async function refreshHotkeys() {
    try {
      setHotkeys(await api.hotkeyStatus());
    } catch {
      setHotkeys(null);
    }
  }

  async function reclaimHotkeys() {
    try {
      await api.quitOtherInstances();
      // Give the other copies a beat to release the hotkeys they own.
      window.setTimeout(() => void api.reclaimHotkeys().then(setHotkeys).catch(() => refreshHotkeys()), 900);
    } catch (error) {
      flash(String(error), 3800);
    }
  }

  async function runAccessCheck() {
    setChecking(true);
    try {
      setChecks(await api.checkOutputAccess());
    } catch (error) {
      flash(String(error), 3800);
    } finally {
      setChecking(false);
    }
  }

  useEffect(() => {
    if (!isTauri()) {
      setSettings(FALLBACK_SETTINGS);
      return;
    }
    void api
      .getSettings()
      .then(setSettings)
      .catch((error) => {
        setSettings(FALLBACK_SETTINGS);
        flash(String(error), 3200);
      });
    void api
      .screenCaptureAccessStatus()
      .then(setScreenAccess)
      .catch(() => setScreenAccess(null));
    void api
      .hotkeyStatus()
      .then(setHotkeys)
      .catch(() => setHotkeys(null));
    const items = getRecentItems();
    void api
      .existingFiles(items.map((item) => item.path))
      .then((existing) => {
        const keep = new Set(existing);
        const pruned = items.filter((item) => keep.has(item.path));
        if (pruned.length !== items.length) {
          setRecentItems(pruned);
          setRecent(pruned);
        }
      })
      .catch(() => undefined);
    // Settings are loaded once when the main window starts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    if (isTauri()) {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          if (event.payload.type === "enter") {
            setDragging(true);
            setDropAction(actionAt(event.payload.position));
          }
          if (event.payload.type === "over") {
            setDropAction(actionAt(event.payload.position));
          }
          if (event.payload.type === "leave") {
            setDragging(false);
            setDropAction(null);
          }
          if (event.payload.type === "drop") {
            const action = actionAt(event.payload.position);
            setDragging(false);
            setDropAction(null);
            void dropFiles(event.payload.paths, action).catch((error) => flash(String(error), 3200));
          }
        })
        .then((unlisten) => unlisteners.push(unlisten));
      void listen<string>("app://error", (event) => flash(event.payload, 3800)).then((unlisten) =>
        unlisteners.push(unlisten),
      );
      void listen("app://open-ocr", () => void chooseOcr()).then((unlisten) =>
        unlisteners.push(unlisten),
      );
      void listen("app://recent-changed", () => setRecent(getRecentItems())).then((unlisten) =>
        unlisteners.push(unlisten),
      );
      void listen<{ outputs: string[]; message: string }>("app://file-result", (event) => {
        const output = event.payload.outputs[0];
        if (!output) return;
        rememberRecent({ path: output, kind: recentKind(output), at: Date.now() });
        setRecent(getRecentItems());
      }).then((unlisten) => unlisteners.push(unlisten));
    }
    const unsubscribeRecent = subscribeRecent(() => setRecent(getRecentItems()));
    // Keeps the relative timestamps honest without re-reading storage constantly.
    const clockTimer = window.setInterval(() => setNowTick((value) => value + 1), 30_000);
    return () => {
      unlisteners.forEach((unlisten) => unlisten());
      unsubscribeRecent();
      window.clearInterval(clockTimer);
      if (toastTimer.current) window.clearTimeout(toastTimer.current);
    };
    // Handlers only use stable setters and imported APIs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const outputLabel = (kind: WriteCheck["kind"]) =>
    kind === "screenshot" ? t("screenshots") : kind === "recording" ? t("recordings") : t("fileResults");

  return (
    <div className={`home ${IS_MAC ? "is-mac" : ""}`}>
      <header className="home-bar" data-tauri-drag-region>
        <div className="home-brand" data-tauri-drag-region>
          <img className="home-mark" src="/shift-icon.png" alt="" aria-hidden="true" />
          <strong>Shift</strong>
          <span className="home-brand-sub">Your Files</span>
        </div>
        <div className="home-bar-actions">
          <button
            className="home-quick"
            title={t("captureFullscreen")}
            onClick={() => void spring("fullscreen", () => api.captureFullscreen().then(() => undefined))}
          >
            {busy === "fullscreen" ? (
              <LoaderCircle size={15} className="spin" />
            ) : (
              <Maximize size={15} />
            )}
            {t("fullScreen")}
            <kbd>{IS_MAC ? "⌥⇧4" : "Alt+Shift+4"}</kbd>
          </button>
          <button
            className="home-quick"
            onClick={() =>
              void spring("capture", () => api.startCapture("screenshot").then(() => undefined))
            }
          >
            {busy === "capture" ? <LoaderCircle size={15} className="spin" /> : <ScanLine size={15} />}
            {t("capture")}
            <kbd>{IS_MAC ? "⌥⇧2" : "Alt+Shift+2"}</kbd>
          </button>
          <button
            className="home-quick"
            onClick={() =>
              void spring("record", () => api.startCapture("recording").then(() => undefined))
            }
          >
            {busy === "record" ? <LoaderCircle size={15} className="spin" /> : <Circle size={14} />}
            {t("record")}
            <kbd>{IS_MAC ? "⌥⇧3" : "Alt+Shift+3"}</kbd>
          </button>
          <button
            className={`home-icon-button ${settingsOpen ? "is-active" : ""}`}
            title={t("settings")}
            onClick={() => setSettingsOpen((value) => !value)}
          >
            <Settings2 size={17} />
          </button>
        </div>
      </header>

      <main className="home-main">
        <section className={`home-drop ${dragging ? "is-dragging" : ""}`}>
          {screenAccess === false ? (
            <div className="permission-banner">
              <ShieldAlert size={16} />
              <span>{t("screenPermissionPrompt")}</span>
              <button onClick={() => void api.openScreenRecordingSettings()}>
                {t("openScreenSettings")}
              </button>
            </div>
          ) : null}
          <div className="home-drop-art" aria-hidden="true">
            {DROP_ICONS.map(({ icon: Icon, tone }, index) => {
              // Place every card on one circle: x follows sin(angle) and the outer
              // cards sink by the sagitta, which reads as a single clean arc.
              const angle = ((index - 3) * ARC_STEP_DEG * Math.PI) / 180;
              return (
                <span
                  key={tone}
                  className={`home-drop-card tone-${tone}`}
                  style={
                    {
                      "--angle": `${((index - 3) * ARC_STEP_DEG).toFixed(1)}deg`,
                      "--x": `${(Math.sin(angle) * ARC_RADIUS).toFixed(1)}px`,
                      "--y": `${(ARC_LIFT - (1 - Math.cos(angle)) * ARC_RADIUS).toFixed(1)}px`,
                      zIndex: 10 - Math.abs(index - 3),
                    } as CSSProperties
                  }
                >
                  <Icon size={23} strokeWidth={1.7} />
                </span>
              );
            })}
          </div>
          <h1>{dragging ? t("homeDropRelease") : t("homeDropTitle")}</h1>
          <p>{t("homeDropBody")}</p>
          <div className={`home-primary-actions ${dropAction ? "has-drop-target" : ""}`}>
            <button
              className={`home-primary ${dropAction === "formats" ? "is-drop-target" : ""}`}
              ref={(node) => {
                dropTargets.current.formats = node;
              }}
              onClick={() =>
                void spring("convert", () => chooseFiles("formats"))
              }
            >
              {busy === "convert" ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <FileOutput size={16} />
              )}
              {t("convertFiles")}
              <ArrowUpRight size={15} />
            </button>
            <button
              className={`home-secondary ${dropAction === "tools" ? "is-drop-target" : ""}`}
              ref={(node) => {
                dropTargets.current.tools = node;
              }}
              onClick={() => void spring("tools", () => chooseFiles("tools"))}
            >
              {busy === "tools" ? (
                <LoaderCircle size={16} className="spin" />
              ) : (
                <Wrench size={15} />
              )}
              {t("fileTools")}
            </button>
            <button
              className={`home-secondary ${dropAction === "ocr" ? "is-drop-target" : ""}`}
              ref={(node) => {
                dropTargets.current.ocr = node;
              }}
              onClick={() => void spring("ocr", chooseOcr)}
            >
              {busy === "ocr" ? <LoaderCircle size={16} className="spin" /> : <ScanText size={16} />}
              {t("ocr")}
            </button>
          </div>
          <div className="home-shortcut-row">
            <span>{dragging ? t("dragHintActive") : t("dragHint")}</span>
            <span className="home-shortcut-group">
              <kbd>{IS_MAC ? "\u21e7" : "Shift"}</kbd>
              {t("dragHintConvert")}
            </span>
            <span className="home-shortcut-group">
              <kbd>{IS_MAC ? "\u2325\u21e7" : "Ctrl+Shift"}</kbd>
              {t("dragHintTools")}
            </span>
          </div>
        </section>

        <aside className="home-side">
          <section className="home-card home-card-recent">
            <header className="home-card-head">
              <Clock3 size={16} />
              <div>
                <span>{t("recent")}</span>
                <h2>
                  {recent.length > 0 ? `${recent.length} ${t("items")}` : t("nothingYet")}
                </h2>
              </div>
            </header>
            {recent.length === 0 ? (
              <div className="home-empty">
                <span className="home-empty-glyph" aria-hidden="true">
                  <ScanLine size={17} />
                </span>
                <p>{t("recentEmpty")}</p>
              </div>
            ) : (
              <div className="home-recent-list">
                {recent.map((item) => (
                  <div key={item.path} className="home-recent" title={item.path}>
                    <button
                      className="home-recent-main"
                      onClick={() => void api.openInEditor(item.path)}
                    >
                      <RecentThumb item={item} />
                      <span>
                        <strong>{basename(item.path)}</strong>
                        <small>
                          {timeAgo(item.at, t)} · {dirname(item.path)}
                        </small>
                      </span>
                    </button>
                    <span className="home-recent-actions">
                      <button title={t("revealFile")} onClick={() => void api.revealFile(item.path)}>
                        <FolderSearch size={13} />
                      </button>
                      {item.kind === "image" ? (
                        <button title={t("pin")} onClick={() => void api.pinImage(item.path)}>
                          <Pin size={13} />
                        </button>
                      ) : null}
                    </span>
                  </div>
                ))}
              </div>
            )}
          </section>

        </aside>
      </main>

      {settingsOpen ? (
        <div className="home-sheet-backdrop" onClick={() => setSettingsOpen(false)} />
      ) : null}

      <aside className={`home-settings ${settingsOpen ? "is-open" : ""}`}>
        <header className="home-settings-head">
          <div>
            <span>{t("settings")}</span>
            <h2>{t("outputSettings")}</h2>
          </div>
          <button className="home-icon-button" onClick={() => setSettingsOpen(false)}>
            <X size={17} />
          </button>
        </header>

        <section className="home-settings-section">
          <div className="home-section-title">
            <FolderCog size={15} />
            {t("fileResults")}
          </div>
          <div className="home-segment">
            <button
              className={settings?.fileOutputMode !== "custom" ? "is-active" : ""}
              onClick={() => void applySettings(() => api.setFileOutputMode("alongside"))}
            >
              {t("nextToOriginal")}
            </button>
            <button
              className={settings?.fileOutputMode === "custom" ? "is-active" : ""}
              onClick={() => void applySettings(() => api.setFileOutputMode("custom"))}
            >
              {t("customFolder")}
            </button>
          </div>
          <p className="home-settings-hint">
            {settings?.fileOutputMode === "custom" ? t("customFolderHint") : t("alongsideHint")}
          </p>
          {settings?.fileOutputMode === "custom" ? (
            <OutputLocation
              label={t("fileResults")}
              path={settings.fileOutputDir}
              chooseLabel={t("chooseFolder")}
              revealLabel={t("revealFolder")}
              resetLabel={t("reset")}
              onChoose={() => void chooseFolder("files")}
              onReveal={() => void api.revealOutputDirectory("files")}
              onReset={() => void resetFolder("files")}
            />
          ) : null}
        </section>

        <section className="home-settings-section">
          <div className="home-section-title">
            <ScanLine size={15} />
            {t("screenshots")}
          </div>
          <OutputLocation
            label={t("location")}
            path={settings?.screenshotDir ?? FALLBACK_SETTINGS.screenshotDir}
            chooseLabel={t("chooseFolder")}
            revealLabel={t("revealFolder")}
            resetLabel={t("reset")}
            onChoose={() => void chooseFolder("screenshot")}
            onReveal={() => void api.revealOutputDirectory("screenshot")}
            onReset={() => void resetFolder("screenshot")}
          />
        </section>

        <section className="home-settings-section">
          <div className="home-section-title">
            <Circle size={14} />
            {t("recordings")}
          </div>
          <OutputLocation
            label={t("location")}
            path={settings?.recordingDir ?? FALLBACK_SETTINGS.recordingDir}
            chooseLabel={t("chooseFolder")}
            revealLabel={t("revealFolder")}
            resetLabel={t("reset")}
            onChoose={() => void chooseFolder("recording")}
            onReveal={() => void api.revealOutputDirectory("recording")}
            onReset={() => void resetFolder("recording")}
          />
        </section>

        <section className="home-settings-section">
          <div className="home-section-title">{t("behaviour")}</div>
          <label className="home-toggle-row">
            <span>
              <strong>{t("dragRings")}</strong>
              <small>{t("dragRingsHint")}</small>
            </span>
            <button
              className={`home-toggle ${settings?.dragEnabled ?? true ? "is-on" : ""}`}
              onClick={() =>
                void applySettings(() => api.setDragEnabled(!(settings?.dragEnabled ?? true)))
              }
              aria-pressed={settings?.dragEnabled ?? true}
            >
              <span />
            </button>
          </label>
          <label className="home-toggle-row">
            <span>
              <strong>{t("autostart")}</strong>
              <small>{t("autostartHint")}</small>
            </span>
            <button
              className={`home-toggle ${settings?.autostart ? "is-on" : ""}`}
              onClick={() => void applySettings(() => api.setAutostart(!(settings?.autostart ?? false)))}
              aria-pressed={settings?.autostart ?? false}
            >
              <span />
            </button>
          </label>
        </section>

        <section className="home-settings-section">
          <label className="home-toggle-row">
            <span>
              <strong>{t("showInFinder")}</strong>
              <small>{t("showInFinderHint")}</small>
            </span>
            <button
              className={`home-toggle ${settings?.autoReveal ? "is-on" : ""}`}
              onClick={() =>
                void applySettings(() => api.setAutoReveal(!(settings?.autoReveal ?? true)))
              }
              aria-pressed={settings?.autoReveal ?? true}
            >
              <span />
            </button>
          </label>
        </section>

        <section className="home-settings-section">
          <div className="home-section-title">
            <Keyboard size={15} />
            {t("hotkeys")}
          </div>
          {hotkeys && (!hotkeys.live || hotkeys.others.length > 0) ? (
            <div className="home-hotkeys is-warn">
              <p className="home-settings-hint">
                {hotkeys.others.length > 0
                  ? t("hotkeysOthers", { count: hotkeys.others.length })
                  : t("hotkeysNotLive")}
              </p>
              <button className="settings-wide-button" onClick={() => void reclaimHotkeys()}>
                <RefreshCw size={14} />
                {t("hotkeysFix")}
              </button>
            </div>
          ) : (
            <p className="home-settings-hint">{t("hotkeysOk")}</p>
          )}
          <div className="home-hotkey-list">
            {(hotkeys?.hotkeys.length
              ? hotkeys.hotkeys
              : ["Alt+Shift+2", "Alt+Shift+3", "Alt+Shift+4"]
            ).map((hotkey) => (
              <span key={hotkey}>{hotkey.replace("Alt+Shift+", `${IS_MAC ? "\u2325\u21e7" : "Alt+Shift+"}`)}</span>
            ))}
          </div>
        </section>

        <section className="home-settings-section">
          <div className="home-section-title">
            <ShieldCheck size={15} />
            {t("accessCheck")}
          </div>
          <p className="home-settings-hint">{t("accessCheckHint")}</p>
          <button className="settings-wide-button" onClick={() => void runAccessCheck()}>
            {checking ? <LoaderCircle size={14} className="spin" /> : <RefreshCw size={14} />}
            {checking ? t("checking") : t("checkAccess")}
          </button>
          {checks ? (
            <div className="home-checks">
              {checks.map((check) => (
                <div key={check.kind} className={`home-check ${check.ok ? "is-ok" : "is-bad"}`}>
                  {check.ok ? <Check size={14} /> : <X size={14} />}
                  <span>
                    <strong>{outputLabel(check.kind)}</strong>
                    <small>{check.ok ? t("accessOk") : check.message}</small>
                  </span>
                </div>
              ))}
            </div>
          ) : null}
        </section>

        {IS_MAC ? (
          <section className="home-settings-section">
            <div className="home-section-title">{t("permissions")}</div>
            <div className="home-permission-row">
              <button onClick={() => void api.openScreenRecordingSettings()}>
                <ScanLine size={14} />
                {t("screenPermission")}
              </button>
              <button onClick={() => void api.openFilesPermissionSettings()}>
                <FolderOpen size={14} />
                {t("filesPermission")}
              </button>
            </div>
            <p className="home-settings-hint">{t("filesPermissionHint")}</p>
          </section>
        ) : null}

        <section className="home-settings-section">
          <div className="home-section-title">{t("language")}</div>
          <div className="home-segment">
            <button className={lang === "en" ? "is-active" : ""} onClick={() => setLang("en")}>
              English
            </button>
            <button className={lang === "zh" ? "is-active" : ""} onClick={() => setLang("zh")}>
              简体中文
            </button>
          </div>
          <p className="home-settings-hint">{t("privacyBody")}</p>
        </section>
      </aside>

      {toast ? <div className="main-toast">{toast}</div> : null}
    </div>
  );
}
