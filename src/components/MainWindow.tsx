import { convertFileSrc, isTauri } from "@tauri-apps/api/core";
import { emitTo, listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  Check,
  Circle,
  Clock3,
  Cpu,
  Download,
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
  LoaderCircle,
  Maximize,
  MoveRight,
  Pin,
  Presentation,
  RefreshCw,
  ScanLine,
  ScanText,
  Search,
  Send,
  Settings2,
  ShieldAlert,
  ShieldCheck,
  Sparkle,
  Star,
  Wrench,
  X,
} from "lucide-react";
import {
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
  type DragEvent,
} from "react";
import { getInitialLang, publishLang, tLive, useI18n } from "../i18n";
import { api } from "../lib/api";
import { FEEDBACK } from "../lib/feedback";
import { hitTestDropTarget, releasePoint, toWindowPoint } from "../lib/dropTargets";
import {
  actionCanServe,
  dragPathsKnown,
  dropEffectFor,
  OCR_IMAGE_EXTENSIONS,
  type DropAction,
} from "../lib/dropCapabilities";
import { SetupPanel } from "./SetupPanel";
import { SharePanel } from "./SharePanel";
import { UpdatePanel } from "./UpdatePanel";
import {
  basename,
  dirname,
  extensionOf,
  getLocalRecentItems,
  IS_MAC,
  messageFrom,
  type RecentItem,
  subscribeRecent,
  timeAgo,
} from "../lib/utils";
import { dragOutPayload } from "../lib/dragOut";
import { fileDragImage } from "../lib/dragImage";
import { history } from "../lib/history";
import type {
  AppSettings,
  HotkeyStatus,
  RuntimeComponent,
  RuntimeInfo,
  ShareTransfer,
  SwitchState,
  SwitchUpdate,
  WriteCheck,
} from "../types";
const FALLBACK_SETTINGS: AppSettings = {
  fileOutputMode: "alongside",
  fileOutputDir: "~/Downloads/Shift",
  screenshotDir: "~/Pictures/Shift",
  recordingDir: "~/Movies/Shift",
  autoReveal: true,
  dragEnabled: true,
  shareEnabled: true,
  autostart: false,
  defaultFileOutputDir: "~/Downloads/Shift",
  defaultScreenshotDir: "~/Pictures/Shift",
  defaultRecordingDir: "~/Movies/Shift",
  language: "",
  updateCheckEnabled: true,
  updateFeedUrl: "",
  wedgeOrders: {},
  setupReviewed: false,
};

type SettingsTab = "system" | "runtime" | "updates";

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

/* The three primary buttons double as drop targets: files dropped on a button run that
   action. Which button a drag may land on is decided by `lib/dropCapabilities`. */

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
        <FileImage size={17} />
      ) : item.kind === "video" ? (
        <FileVideo size={17} />
      ) : item.kind === "text" ? (
        <FileText size={17} />
      ) : (
        <FileGeneric size={17} />
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
        <FolderOpen size={16} />
        <span>{path}</span>
      </div>
      <div className="settings-output-actions">
        <button className="settings-small-button" onClick={onChoose}>
          <FolderSearch size={16} />
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
  const [languageError, setLanguageError] = useState<string | null>(null);
  // Every switch uses this snapshot until the real settings arrive. Starting
  // empty made the first click wait for a disk read before it could change.
  const [settings, setSettings] = useState<AppSettings | null>(FALLBACK_SETTINGS);
  const [setupOpen, setSetupOpen] = useState(false);
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settingsTab, setSettingsTab] = useState<SettingsTab>("system");
  const [screenAccess, setScreenAccess] = useState<boolean | null>(null);
  const [checks, setChecks] = useState<WriteCheck[] | null>(null);
  const [checking, setChecking] = useState(false);
  const [runtimeInfo, setRuntimeInfo] = useState<RuntimeInfo | null>(null);
  const [runtimeLoading, setRuntimeLoading] = useState(false);
  const [runtimeInstalling, setRuntimeInstalling] = useState<string[]>([]);
  const [runtimeError, setRuntimeError] = useState<string | null>(null);
  const [runtimeInstallHelp, setRuntimeInstallHelp] = useState<
    "homebrew" | "winget" | null
  >(null);
  const [recent, setRecent] = useState<RecentItem[]>(getLocalRecentItems);
  /** Filter text for the history library, matched against the file name. */
  const [historyQuery, setHistoryQuery] = useState("");
  const [sharePaths, setSharePaths] = useState<string[] | null>(null);
  const toastTimer = useRef<number | null>(null);
  const [, setNowTick] = useState(0);
  const [dropAction, setDropAction] = useState<DropAction | null>(null);
  /** True while a Recent row is being dragged, which the native `dragging` never covers. */
  const [rowDragging, setRowDragging] = useState(false);
  /**
   * The files the drag in flight is carrying, so a button can say "not this one"
   * before the file is released. Both drag sources report them: the native event
   * on `enter`, a Recent row on `dragstart`.
   */
  const [dragPaths, setDragPaths] = useState<string[] | null>(null);
  /**
   * Counts the drags this window has seen, so the cursor's verdict is handed to the
   * native layer once per gesture as well. The native side opens every session permissive,
   * which means the report it is given has to come after that - and only the arrival of a
   * drag says when that is.
   */
  const [dragSession, setDragSession] = useState(0);
  const [hotkeys, setHotkeys] = useState<HotkeyStatus | null>(null);
  const dropTargets = useRef<Partial<Record<DropAction, HTMLElement | null>>>({});
  /**
   * The Recent row that is in flight, and whether it has already landed. A row leaves
   * Shift as a real file drag, so one gesture can be reported twice: once by the DOM,
   * once by the native drop events. Only the first of the two may run an action.
   */
  const recentDrag = useRef<{ path: string; landed: boolean } | null>(null);
  /**
   * The last position the drag in flight moved through, in CSS pixels inside this
   * window. Chromium reports `dragend` at the origin rather than where the pointer
   * stopped, so this is the only aim that engine can be given.
   */
  const dragPointer = useRef<{ x: number; y: number } | null>(null);
  const runtimeCheckStarted = useRef(false);

  async function finishSetup() {
    setSetupOpen(false);
    try {
      await api.setSetupReviewed(true);
    } catch {
      // A failed write only means the flow may appear again next launch; the
      // checks it reported are all re-runnable from Settings.
    }
  }

  /** Leave the guided flow and land on the panel that can install what it found. */
  function openRuntimeFromSetup() {
    void finishSetup();
    setSettingsTab("runtime");
    setSettingsOpen(true);
  }

  async function flash(message: string, duration: number = FEEDBACK.notice) {
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
      flash(t(messageFrom(error)), FEEDBACK.error);
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
          extensions: OCR_IMAGE_EXTENSIONS,
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
    // The button already showed it could not take this file, so a release on it
    // stays silent: no action, no message, nothing to acknowledge.
    if (!actionCanServe("ocr", paths)) return;
    await api.openInEditor(paths[0]);
    window.setTimeout(() => {
      void emitTo("editor", "editor://run-ocr", {});
    }, 450);
  }

  function localPoint(position: { x: number; y: number }) {
    return toWindowPoint(position, IS_MAC ? "mac" : "other", window.devicePixelRatio || 1);
  }

  /** Hit-test a point that is already in CSS pixels inside this window. */
  function actionAtPoint(point: { x: number; y: number }): DropAction | null {
    const boxes: Array<[DropAction, DOMRect | null | undefined]> = [
      ["formats", dropTargets.current.formats?.getBoundingClientRect()],
      ["tools", dropTargets.current.tools?.getBoundingClientRect()],
      ["ocr", dropTargets.current.ocr?.getBoundingClientRect()],
    ];
    return hitTestDropTarget(boxes, point);
  }

  function actionAt(position: { x: number; y: number }): DropAction | null {
    const point = localPoint(position);
    if (point) dragPointer.current = point;
    return point ? actionAtPoint(point) : null;
  }

  /**
   * Run the one action a drop was aimed at.
   *
   * The button *is* the choice. `null` means the file was released on the panel
   * rather than on a button, and that runs nothing and says nothing - the same
   * nothing a row released off a button does, which is why a file from outside and
   * a row from the list are now one gesture with one set of consequences.
   */
  function dropFiles(paths: string[], action: DropAction | null) {
    if (paths.length === 0) return Promise.resolve();
    if (!action) return Promise.resolve();
    // The same rule the button's appearance already applied: a button that cannot
    // serve this file runs nothing, rather than opening a wheel with nothing in it.
    if (!actionCanServe(action, paths)) return Promise.resolve();
    if (action === "ocr") return runOcrOn(paths);
    return api.showFilesInRadial(paths, action).then(() => undefined);
  }

  /**
   * Land a dragged Recent row, at most once per gesture.
   */
  function landRecentRow(action: DropAction | null, token?: { path: string; landed: boolean }) {
    const drag = recentDrag.current;
    if (!drag || drag.landed) return;
    // A timer from the gesture before this one must not land it on the row's behalf.
    if (token && token !== drag) return;
    drag.landed = true;
    setDropAction(null);
    // The gesture is over, whether or not `dragend` arrives to say so.
    dragPointer.current = null;
    setDragPaths(null);
    void dropFiles([drag.path], action).catch((error) => flash(tLive(messageFrom(error)), FEEDBACK.error));
  }

  /**
   * DOM half of the same gesture, armed on the window root. Whether a file drag that
   * started inside the webview is reported here, to the native handler, or to both
   * differs by engine, so both are listening and `landRecentRow` decides who was first.
   */
  function rowDragOver(event: DragEvent) {
    const drag = recentDrag.current;
    if (!drag) return;
    event.preventDefault();
    const point = { x: event.clientX, y: event.clientY };
    dragPointer.current = point;
    const next = actionAtPoint(point);
    // A spent button must show the same refusal before release that it shows on
    // release: `none` turns the operating system's cursor into a prohibition.
    event.dataTransfer.dropEffect = dropEffectFor(next, [drag.path]);
    if (dropAction !== next) setDropAction(next);
  }

  function rowDropped(event: DragEvent) {
    if (!recentDrag.current) return;
    event.preventDefault();
    landRecentRow(actionAtPoint({ x: event.clientX, y: event.clientY }));
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
      flash(t(messageFrom(error)), FEEDBACK.error);
    }
  }

  // Toggles flip their visual state at once and reconcile with the backend
  // snapshot afterwards, so a slow disk or login-item call cannot stall the switch.
  async function applyToggle(
    patch: Partial<AppSettings>,
    task: () => Promise<SwitchState | SwitchUpdate>,
  ) {
    const previous = settings;
    if (previous) setSettings({ ...previous, ...patch });
    try {
      const response = await task();
      setSettings((current) => {
        if (!current) return current;
        const enabled =
          "state" in response ? response.state.enabled : response.enabled;
        return {
          ...current,
          ...patch,
          ...("autostart" in response && response.autostart !== null
            ? { autostart: response.autostart }
            : { autostart: "autostart" in patch ? enabled : current.autostart }),
          dragEnabled: "dragEnabled" in patch ? enabled : current.dragEnabled,
          shareEnabled: "shareEnabled" in patch ? enabled : current.shareEnabled,
          autoReveal: "autoReveal" in patch ? enabled : current.autoReveal,
        };
      });
    } catch (error) {
      if (previous) setSettings(previous);
      flash(t(messageFrom(error)), FEEDBACK.error);
    }
  }

  async function changeLanguage(next: "en" | "zh") {
    setLanguageError(null);
    // publishLang persists the choice and notifies Rust exactly once. `setLang`
    // also updates this webview, so the switch does not wait for that IPC.
    setLang(next);
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

  async function refreshRuntime() {
    if (!isTauri()) return;
    setRuntimeLoading(true);
    setRuntimeError(null);
    setRuntimeInstallHelp(null);
    try {
      setRuntimeInfo(await api.getRuntimeInfo());
    } catch (error) {
      setRuntimeError(t(messageFrom(error)));
    } finally {
      setRuntimeLoading(false);
    }
  }

  function describeRuntimeInstallError(error: unknown) {
    const message = messageFrom(error);
    setRuntimeInstallHelp(
      message === "homebrew-missing"
        ? "homebrew"
        : message === "winget-missing" || message === "winget-unavailable"
          ? "winget"
          : null,
    );
    if (message === "homebrew-missing") {
      return t("runtimeInstallNeedsHomebrew");
    }
    if (message === "winget-missing") {
      return t("runtimeInstallNeedsWinget");
    }
    if (
      message === "runtime-install-unsupported" ||
      message === "unsupported-runtime-component"
    ) {
      return t("runtimeInstallUnsupported");
    }
    if (message.startsWith("homebrew-install-failed:")) {
      // The tail is the installer's own exit status - worth having in a log,
      // useless as a sentence a user has to read.
      console.error(message);
      return t("runtimeInstallFailed");
    }
    if (
      message === "homebrew-install-missing-binary" ||
      message === "winget-install-missing-binary"
    ) {
      // The installer reported success and Shift still cannot see the tool. On Windows
      // that is nearly always a PATH the running process has not re-read, and it is
      // fixed by restarting Shift rather than by pressing Install again.
      if (message === "winget-install-missing-binary") return t("runtimeInstallNotOnPath");
      return t("runtimeInstallFailed");
    }
    // Windows names each shape of failure it can distinguish, because "installation
    // failed" cannot tell a closed UAC prompt from a dead network - and only one of
    // those is fixed by pressing the same button again.
    if (message === "winget-elevation-declined") return t("runtimeInstallNeedsAdmin");
    if (message === "winget-unavailable") return t("runtimeInstallNoPackageManager");
    if (message === "winget-install-timeout") return t("runtimeInstallTimedOut");
    if (message === "winget-install-failed" || message === "winget-launch-failed") {
      return t("runtimeInstallFailed");
    }
    // Every failure the installer raises is a code it names here, so anything
    // still unrecognised is worded by the dictionary or shown as it came.
    return t(message);
  }

  async function installRuntimeComponents(components: RuntimeComponent[]) {
    if (runtimeInstalling.length > 0 || components.length === 0) return;
    setRuntimeInstalling(components.map((component) => component.id));
    setRuntimeError(null);
    setRuntimeInstallHelp(null);
    try {
      await api.installRuntimeComponents(components.map((component) => component.id));
      flash(
        components.length === 1
          ? t("runtimeInstallComplete", { name: components[0].name })
          : t("runtimeInstallManyComplete", { count: components.length }),
        FEEDBACK.done,
      );
    } catch (error) {
      setRuntimeError(describeRuntimeInstallError(error));
    } finally {
      setRuntimeInstalling([]);
      // A batch can stop at its first failure with the earlier installs already
      // on disk, so the panel re-reads either way rather than trusting the reply.
      await refreshRuntime();
    }
  }

  async function reclaimHotkeys() {
    try {
      await api.quitOtherInstances();
      // Give the other copies a beat to release the hotkeys they own.
      window.setTimeout(() => void api.reclaimHotkeys().then(setHotkeys).catch(() => refreshHotkeys()), 900);
    } catch (error) {
      flash(t(messageFrom(error)), FEEDBACK.error);
    }
  }

  async function runAccessCheck() {
    setChecking(true);
    try {
      setChecks(await api.checkOutputAccess());
    } catch (error) {
      flash(t(messageFrom(error)), FEEDBACK.error);
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
      .then((loaded) => {
        setSettings(loaded);
        // The tray menu is native, so it can only show the language the backend
        // knows about. Adopt the stored choice, or hand it ours on a first run;
        // the backend only writes when the value actually changed.
        if (!loaded.setupReviewed) setSetupOpen(true);
        const stored = loaded.language;
        const effective = stored === "en" || stored === "zh" ? stored : getInitialLang();
        if (effective !== stored) publishLang(effective);
      })
      .catch((error) => {
        setSettings(FALLBACK_SETTINGS);
        flash(t(messageFrom(error)), FEEDBACK.error);
      });
    void api
      .screenCaptureAccessStatus()
      .then(setScreenAccess)
      .catch(() => setScreenAccess(null));
    void api
      .hotkeyStatus()
      .then(setHotkeys)
      .catch(() => setHotkeys(null));
    // Rust prunes entries whose file is gone as it reads the library, so there is
    // no separate sweep to run here.
    void history.adopt().then(setRecent).catch(() => undefined);
    // Settings are loaded once when the main window starts.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const onLanguageError = (event: Event) => {
      const detail = (event as CustomEvent<string>).detail;
      setLanguageError(detail || t("languageSaveFailed"));
    };
    window.addEventListener("shift:language-error", onLanguageError);
    return () => window.removeEventListener("shift:language-error", onLanguageError);
  }, [t]);

  // Reading the status also consumes a "second copy tried to start" marker.
  useEffect(() => {
    if (settingsOpen && isTauri()) void refreshHotkeys();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [settingsOpen]);

  // Every dismissible surface answers to Escape: the send panel already did, and
  // the settings sheet only answered to its close button and its backdrop. The
  // panel owns the key while it is open, so one press never closes two things.
  useEffect(() => {
    if (!settingsOpen) return;
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || sharePaths || setupOpen) return;
      setSettingsOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [settingsOpen, sharePaths, setupOpen]);

  // Announce files that another Shift pushed to this machine. A dedicated effect so
  // the message re-subscribes when the language changes.
  useEffect(() => {
    if (!isTauri()) return;
    const unlisten = listen<ShareTransfer>("share://transfer", (event) => {
      const transfer = event.payload;
      if (transfer.state === "received" && transfer.path) {
        flash(t("shareReceived", { name: transfer.fileName, peer: transfer.peerName }), FEEDBACK.notice);
      } else if (transfer.state === "error") {
        flash(
          transfer.fileName
            ? t("shareReceiveFailed", {
                name: transfer.fileName,
                peer: transfer.peerName || t("sharePeerFallback"),
              })
            : t("shareReceiveFailedGeneric"),
          FEEDBACK.error,
        );
      }
    });
    return () => {
      void unlisten.then((release) => release());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [t]);

  useEffect(() => {
    if (!isTauri() || runtimeCheckStarted.current) return;
    runtimeCheckStarted.current = true;
    // Probe while the home screen is idle so opening Runtime components does not
    // wait for every external tool to start once.
    const timer = window.setTimeout(() => void refreshRuntime(), 600);
    return () => window.clearTimeout(timer);
    // Automatic detection runs only once; later checks come from the refresh button.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  useEffect(() => {
    const unlisteners: Array<() => void> = [];
    if (isTauri()) {
      void getCurrentWebview()
        .onDragDropEvent((event) => {
          const drag = recentDrag.current;
          if (event.payload.type === "enter") {
            setDragging(true);
            setDragSession((value) => value + 1);
            // `enter` is the first moment the carried files are known, which is
            // early enough to mark a button that cannot serve them.
            const carried = event.payload.paths;
            setDragPaths((current) => dragPathsKnown(current, carried));
            setDropAction(actionAt(event.payload.position));
            return;
          }
          if (event.payload.type === "over") {
            setDropAction(actionAt(event.payload.position));
            return;
          }
          if (event.payload.type === "leave") {
            setDragging(false);
            setDropAction(null);
            if (!drag) setDragPaths(null);
            return;
          }
          if (event.payload.type === "drop") {
            const action = actionAt(event.payload.position);
            setDragging(false);
            setDropAction(null);
            setDragPaths(null);
            // A Recent row is a real file drag, so it can reach here as well as the
            // panel's own DOM handler; nothing else can be in flight at the same time.
            if (drag) {
              landRecentRow(action);
              return;
            }
            void dropFiles(event.payload.paths, action).catch((error) => flash(tLive(messageFrom(error)), FEEDBACK.error));
            return;
          }
        })
        .then((unlisten) => unlisteners.push(unlisten));
      // A refused drag ends without a drop and without a leave: the release is the only
      // sign it is over, and the native layer is the only thing that sees it.
      void listen("drop://cancelled", () => {
        setDragging(false);
        setDropAction(null);
        setDragPaths(null);
      }).then((unlisten) => unlisteners.push(unlisten));
      void listen<string>("app://error", (event) => flash(tLive(event.payload), FEEDBACK.error)).then((unlisten) =>
        unlisteners.push(unlisten),
      );
      void listen("app://open-ocr", () => void chooseOcr()).then((unlisten) =>
        unlisteners.push(unlisten),
      );
      // The tray's update row lands here: open Settings on the Updates tab so the
      // download and its SHA256 are the next thing the user sees.
      void listen("app://open-updates", () => {
        setSettingsTab("updates");
        setSettingsOpen(true);
      }).then((unlisten) => unlisteners.push(unlisten));
      // A capture card's Send button: this window owns the peer list, so the card
      // asks here rather than duplicating the discovery UI.
      void listen<{ paths: string[] }>("app://send-files", (event) =>
        setSharePaths(event.payload.paths),
      ).then((unlisten) => unlisteners.push(unlisten));
    }
    const unsubscribeRecent = subscribeRecent(() => void history.list().then(setRecent));
    // Rust owns the library, so its broadcast is the only cross-window signal needed.
    const unsubscribeHistory = history.onChanged(setRecent);
    // Keeps the relative timestamps honest without re-reading storage constantly.
    // The same slow tick keeps relative timestamps and the hotkey-ownership
    // notice honest, so a copy that quits clears the warning without a restart.
    const clockTimer = window.setInterval(() => {
      setNowTick((value) => value + 1);
      if (isTauri()) void refreshHotkeys();
    }, 30_000);
    return () => {
      unlisteners.forEach((unlisten) => unlisten());
      unsubscribeRecent();
      unsubscribeHistory();
      window.clearInterval(clockTimer);
      if (toastTimer.current) window.clearTimeout(toastTimer.current);
    };
    // Handlers only use stable setters and imported APIs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // The library holds hundreds of rows, so the filter runs where the list is
  // rendered rather than as a second request to Rust.
  const visibleRecent = useMemo(() => {
    const needle = historyQuery.trim().toLowerCase();
    if (!needle) return recent;
    return recent.filter((item) => basename(item.path).toLowerCase().includes(needle));
  }, [recent, historyQuery]);

  // One rule for all three buttons and both platforms: a button that cannot serve the
  // files in flight is switched off for the length of the gesture, and a release on it
  // does nothing. The cursor says the same thing, because a refusal that only arrives
  // after the file is let go reads as a dropped action: the button that can serve the
  // drag keeps the copy badge AppKit paints for a willing destination, and the one that
  // cannot takes the prohibition sign instead.
  const spent = (action: DropAction) =>
    dragPaths !== null && !actionCanServe(action, dragPaths);
  // A button that cannot serve the drag is not a candidate, so it never lights up.
  const aimedAction = dropAction && actionCanServe(dropAction, dragPaths) ? dropAction : null;
  // The same rule answers for the cursor: `none` where the pointer sits on a button that
  // cannot serve the drag, or on the panel, which runs nothing either. With no drag in
  // flight the answer is yes, because the next gesture must never start out refused.
  const dragInFlight = dragging || rowDragging || dropAction !== null;
  const dropRefused = dragInFlight && dropEffectFor(dropAction, dragPaths) === "none";
  // Hand the verdict to the native layer: for a file from the file manager the drag never
  // reaches the DOM on macOS, so only the native report can turn the cursor into the
  // prohibition sign. See `src-tauri/src/drop_cursor.rs`.
  useEffect(() => {
    if (!isTauri()) return;
    void api.setDropRefusal(dropRefused);
  }, [dropRefused, dragSession]);

  const outputLabel = (kind: WriteCheck["kind"]) =>
    kind === "screenshot" ? t("screenshots") : kind === "recording" ? t("recordings") : t("fileResults");
  const runtimeGroups = runtimeInfo
    ? [
        { id: "documents", label: t("runtimeDocuments") },
        { id: "graphics", label: t("runtimeGraphics") },
        { id: "video", label: t("runtimeVideo") },
        { id: "capture", label: t("runtimeCapture") },
        { id: "ocr", label: t("runtimeOcr") },
      ]
        .map((group) => ({
          ...group,
          components: runtimeInfo.components.filter(
            (component) => component.category === group.id,
          ),
        }))
        .filter((group) => group.components.length > 0)
    : [];
  // What the panel can fix by itself, in the order the rows appear. A shared package
  // (Poppler answers both PDF rows on Windows) is listed twice here; the backend
  // installs it once.
  const runtimeInstallable = runtimeInfo
    ? runtimeInfo.components.filter((component) => !component.available && component.installable)
    : [];

  return (
    <div
      className={`home ${IS_MAC ? "is-mac" : ""}`}
      onDragOver={rowDragOver}
      onDrop={rowDropped}
    >
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
              <LoaderCircle size={17} className="spin" />
            ) : (
              <Maximize size={17} />
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
            {busy === "capture" ? <LoaderCircle size={17} className="spin" /> : <ScanLine size={17} />}
            {t("capture")}
            <kbd>{IS_MAC ? "⌥⇧2" : "Alt+Shift+2"}</kbd>
          </button>
          <button
            className="home-quick"
            onClick={() =>
              void spring("record", () => api.startCapture("recording").then(() => undefined))
            }
          >
            {busy === "record" ? <LoaderCircle size={17} className="spin" /> : <Circle size={16} />}
            {t("record")}
            <kbd>{IS_MAC ? "⌥⇧3" : "Alt+Shift+3"}</kbd>
          </button>
          <button
            className={`home-icon-button ${settingsOpen ? "is-active" : ""}`}
            title={t("settings")}
            onClick={() => setSettingsOpen((value) => !value)}
          >
            <Settings2 size={19} />
          </button>
        </div>
      </header>

      {setupOpen ? (
        <SetupPanel
          onDone={() => void finishSetup()}
          onOpenRuntime={openRuntimeFromSetup}
        />
      ) : null}

      <main className="home-main">
        <section className="home-drop">
          {screenAccess === false ? (
            <div className="permission-banner" role="alert">
              <ShieldAlert size={16} />
              <span>{t("screenPermissionPrompt")}</span>
              <button onClick={() => void api.openScreenRecordingSettings()}>
                {t("openScreenSettings")}
              </button>
            </div>
          ) : hotkeys && !hotkeys.live ? (
            <div className="permission-banner" role="alert">
              <ShieldAlert size={16} />
              <span>{hotkeys.others.length > 0 ? t("hotkeysDead") : t("hotkeysNotLive")}</span>
              <button onClick={() => void reclaimHotkeys()}>{t("hotkeysFix")}</button>
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
          <h1>{t("homeDropTitle")}</h1>
          {/* The buttons are the only thing a drag reacts to: while one is in flight
              all three show as candidates, so the aim is visible before the pointer
              reaches one, and the panel itself never animates. */}
          <div
            className={`home-primary-actions ${
              dragging || rowDragging || dropAction ? "has-drop-target" : ""
            }`}
          >
            <button
              className={`home-primary ${aimedAction === "formats" ? "is-drop-target" : ""} ${
                spent("formats") ? "is-drop-unavailable" : ""
              }`}
              disabled={spent("formats")}
              ref={(node) => {
                dropTargets.current.formats = node;
              }}
              onClick={() =>
                void spring("convert", () => chooseFiles("formats"))
              }
            >
              {busy === "convert" ? (
                <LoaderCircle size={18} className="spin" />
              ) : (
                <FileOutput size={18} />
              )}
              {t("convertFiles")}
            </button>
            <button
              className={`home-secondary ${aimedAction === "tools" ? "is-drop-target" : ""} ${
                spent("tools") ? "is-drop-unavailable" : ""
              }`}
              disabled={spent("tools")}
              ref={(node) => {
                dropTargets.current.tools = node;
              }}
              onClick={() => void spring("tools", () => chooseFiles("tools"))}
            >
              {busy === "tools" ? (
                <LoaderCircle size={18} className="spin" />
              ) : (
                <Wrench size={18} />
              )}
              {t("fileTools")}
            </button>
            <button
              className={`home-secondary ${aimedAction === "ocr" ? "is-drop-target" : ""} ${
                spent("ocr") ? "is-drop-unavailable" : ""
              }`}
              disabled={spent("ocr")}
              ref={(node) => {
                dropTargets.current.ocr = node;
              }}
              onClick={() => void spring("ocr", chooseOcr)}
            >
              {busy === "ocr" ? <LoaderCircle size={18} className="spin" /> : <ScanText size={18} />}
              {t("ocr")}
            </button>
          </div>
          <p className="home-drop-hint">
            <MoveRight size={14} />
            {t("homeDropHint")}
          </p>
        </section>

        <aside className="home-side">
          <section className="home-card home-card-recent">
            <header className="home-card-head">
              <Clock3 size={18} />
              <h1>{t("recent")}</h1>
              <span className="home-card-count">
                {recent.length > 0
                  ? t("itemCount", {
                      count: historyQuery.trim() ? visibleRecent.length : recent.length,
                    })
                  : t("nothingYet")}
              </span>
              {recent.length > 0 ? (
                <button
                  className="settings-link home-card-clear"
                  onClick={() => void history.clear().catch(() => undefined)}
                >
                  {t("clearHistory")}
                </button>
              ) : null}
            </header>
            {recent.length > 3 ? (
              <label className="home-recent-search">
                <Search size={15} />
                <input
                  value={historyQuery}
                  placeholder={t("searchHistory")}
                  onChange={(event) => setHistoryQuery(event.target.value)}
                />
                {historyQuery ? (
                  <button
                    onClick={() => setHistoryQuery("")}
                    title={t("clearHistory")}
                    aria-label={t("clearHistory")}
                  >
                    <X size={13} />
                  </button>
                ) : null}
              </label>
            ) : null}
            {recent.length === 0 ? (
              <div className="home-empty">
                <span className="home-empty-glyph" aria-hidden="true">
                  <ScanLine size={19} />
                </span>
                <p>{t("recentEmpty")}</p>
              </div>
            ) : visibleRecent.length === 0 ? (
              <div className="home-empty">
                <p>{t("historyNoMatches", { query: historyQuery.trim() })}</p>
              </div>
            ) : (
              <ul className="home-recent-list">
                {visibleRecent.map((item) => (
                  <li
                    key={item.path}
                    className={`home-recent ${item.pinned ? "is-pinned" : ""}`}
                    title={item.path}
                    draggable
                    onDragStart={(event) => {
                      // A row leaves Shift as the file itself: WebKit turns the file URL
                      // in text/uri-list into a real file drop for Finder and friends,
                      // while the plain path remains readable for editors. The same path
                      // is what tells our own drop zones the row has come back.
                      event.dataTransfer.effectAllowed = "copy";
                      recentDrag.current = { path: item.path, landed: false };
                      dragPointer.current = null;
                      setRowDragging(true);
                      setDragSession((value) => value + 1);
                      setDragPaths([item.path]);
                      // Carry the file, not the row. The engine would otherwise photograph
                      // the row itself - thumbnail, name, age, folder and the five hover
                      // buttons - which reads as the list being dragged rather than one
                      // file. The thumbnail the list already decoded is the picture.
                      fileDragImage(
                        event.dataTransfer,
                        basename(item.path),
                        event.currentTarget.querySelector("img"),
                      );
                      // A ring left open from the last drop sits over these buttons,
                      // borderless and always on top. Windows hands a drag to whichever
                      // window is under the pointer, so that ring would claim this drop
                      // and the button underneath would never see it.
                      if (isTauri()) void api.standDownRadial().catch(() => undefined);
                      for (const [type, value] of Object.entries(dragOutPayload(item.path))) {
                        event.dataTransfer.setData(type, value);
                      }
                    }}
                    onDragEnd={(event) => {
                      const drag = recentDrag.current;
                      // `dragend` is the last event of the gesture on some engines and the
                      // first on others: macOS reports the native drop *after* the button
                      // is up, so landing here immediately would consume the gesture with
                      // dragend's own coordinates and then throw away the real drop. Hand
                      // it to a short timer instead - if the DOM drop or the native drop
                      // arrives first it marks the gesture landed and this stands down.
                      // The timer is what makes a row work on Windows, where WebView2's
                      // OLE target accepts only `CF_HDROP`, so neither drop path is
                      // delivered for an in-page drag and the gesture would otherwise end
                      // with nothing at all having happened.
                      if (drag && !drag.landed) {
                        const end = releasePoint(
                          { x: event.clientX, y: event.clientY },
                          dragPointer.current,
                          { width: window.innerWidth, height: window.innerHeight },
                        );
                        const aimed = end ? actionAtPoint(end) : null;
                        // Only an aim at a button is worth landing. When the pointer is
                        // not over one, standing back is right either way: a real drop may
                        // still be on its way, and a release off a button runs nothing.
                        if (aimed) {
                          const token = drag;
                          window.setTimeout(() => landRecentRow(aimed, token), 260);
                        }
                      }
                      setDropAction(null);
                      setDragging(false);
                      setRowDragging(false);
                      setDragPaths(null);
                      // The gesture stays recognised a little longer than the fallback
                      // needs: an engine that reports the drop after `dragend` must still
                      // find it here, so it marks the gesture landed rather than reading
                      // the same file as a fresh drop from outside the app.
                      if (!drag) return;
                      window.setTimeout(() => {
                        if (recentDrag.current === drag) recentDrag.current = null;
                      }, 500);
                    }}
                  >
                    <button
                      className="home-recent-main"
                      onClick={() => void api.openInEditor(item.path)}
                      title={t("open")}
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
                      <button
                        title={item.pinned ? t("unpinFromRecent") : t("pinToRecent")}
                        aria-pressed={item.pinned}
                        onClick={() =>
                          void history.setPinned(item.path, !item.pinned).catch(() => undefined)
                        }
                      >
                        <Star size={15} fill={item.pinned ? "currentColor" : "none"} />
                      </button>
                      <button title={t("sendTo")} onClick={() => setSharePaths([item.path])}>
                        <Send size={15} />
                      </button>
                      <button title={t("revealFile")} onClick={() => void api.revealFile(item.path)}>
                        <FolderSearch size={15} />
                      </button>
                      {item.kind === "image" ? (
                        <button title={t("pin")} onClick={() => void api.pinImage(item.path)}>
                          <Pin size={15} />
                        </button>
                      ) : null}
                      <button
                        title={t("forget")}
                        onClick={() => void history.remove(item.path).catch(() => undefined)}
                      >
                        <X size={15} />
                      </button>
                    </span>
                  </li>
                ))}
              </ul>
            )}
          </section>

        </aside>
      </main>

      {settingsOpen ? (
        <div className="home-sheet-backdrop" onClick={() => setSettingsOpen(false)} />
      ) : null}

      <aside className={`home-settings ${settingsOpen ? "is-open" : ""}`}>
        <header className="home-settings-head">
          <button
            className="home-icon-button home-settings-close"
            onClick={() => setSettingsOpen(false)}
            aria-label={t("close")}
            title={t("close")}
          >
            <X size={19} />
          </button>
          <div className="home-settings-heading">
            <h2>
              {settingsTab === "system"
                ? t("systemSettings")
                : settingsTab === "runtime"
                  ? t("runtimeComponents")
                  : t("updates")}
            </h2>
          </div>
        </header>

        <div className="home-settings-tabs" role="tablist" aria-label={t("settings")}>
          <button
            id="settings-tab-system"
            className={`home-settings-tab ${settingsTab === "system" ? "is-active" : ""}`}
            role="tab"
            aria-selected={settingsTab === "system"}
            aria-controls="settings-panel-system"
            onClick={() => setSettingsTab("system")}
          >
            <Settings2 size={16} />
            {t("systemSettings")}
          </button>
          <button
            id="settings-tab-runtime"
            className={`home-settings-tab ${settingsTab === "runtime" ? "is-active" : ""}`}
            role="tab"
            aria-selected={settingsTab === "runtime"}
            aria-controls="settings-panel-runtime"
            onClick={() => setSettingsTab("runtime")}
          >
            <Cpu size={16} />
            {t("runtimeComponents")}
          </button>
          <button
            id="settings-tab-updates"
            className={`home-settings-tab ${settingsTab === "updates" ? "is-active" : ""}`}
            role="tab"
            aria-selected={settingsTab === "updates"}
            aria-controls="settings-panel-updates"
            onClick={() => setSettingsTab("updates")}
          >
            <Sparkle size={16} />
            {t("updates")}
          </button>
        </div>

        <div className="home-settings-body">
          {settingsTab === "updates" ? (
            <div
              className="home-settings-panel"
              id="settings-panel-updates"
              role="tabpanel"
              aria-labelledby="settings-tab-updates"
            >
              <UpdatePanel
                feedUrl={settings?.updateFeedUrl ?? ""}
                autoCheck={settings?.updateCheckEnabled ?? true}
                onAutoCheckChange={(enabled) =>
                  void applyToggle({ updateCheckEnabled: enabled }, () =>
                    api.setUpdateCheckEnabled(enabled),
                  )
                }
                system={
                  runtimeInfo
                    ? {
                        platform: runtimeInfo.platform,
                        osVersion: runtimeInfo.osVersion,
                        architecture: runtimeInfo.architecture,
                        appVersion: runtimeInfo.appVersion,
                      }
                    : null
                }
              />
            </div>
          ) : settingsTab === "system" ? (
            <div
              className="home-settings-panel"
              id="settings-panel-system"
              role="tabpanel"
              aria-labelledby="settings-tab-system"
            >
              <section className="home-settings-section">
                <div className="home-section-title">
                  <FolderCog size={17} />
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
                  <ScanLine size={17} />
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
                  <Circle size={16} />
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
                <label className="home-toggle-row">
                  <strong>{t("dragRings")}</strong>
                  <button
                    className={`home-toggle ${settings?.dragEnabled ?? true ? "is-on" : ""}`}
                    onClick={() =>
                      void applyToggle(
                        { dragEnabled: !(settings?.dragEnabled ?? true) },
                        () => api.setDragEnabled(!(settings?.dragEnabled ?? true)),
                      )
                    }
                    aria-pressed={settings?.dragEnabled ?? true}
                  >
                    <span />
                  </button>
                </label>
                <label className="home-toggle-row home-toggle-divider">
                  <strong>{t("shiftDrop")}</strong>
                  <button
                    className={`home-toggle ${settings?.shareEnabled ?? true ? "is-on" : ""}`}
                    onClick={() =>
                      void applyToggle(
                        { shareEnabled: !(settings?.shareEnabled ?? true) },
                        () => api.setShareEnabled(!(settings?.shareEnabled ?? true)),
                      )
                    }
                    aria-pressed={settings?.shareEnabled ?? true}
                  >
                    <span />
                  </button>
                </label>
                <label className="home-toggle-row home-toggle-divider">
                  <strong>{t("autostart")}</strong>
                  <button
                    className={`home-toggle ${settings?.autostart ? "is-on" : ""}`}
                    onClick={() =>
                      void applyToggle(
                        { autostart: !(settings?.autostart ?? false) },
                        () => api.setAutostart(!(settings?.autostart ?? false)),
                      )
                    }
                    aria-pressed={settings?.autostart ?? false}
                  >
                    <span />
                  </button>
                </label>
              </section>

              <section className="home-settings-section">
                <label className="home-toggle-row">
                  <strong>{t("showInFinder")}</strong>
                  <button
                    className={`home-toggle ${settings?.autoReveal ? "is-on" : ""}`}
                    onClick={() =>
                      void applyToggle(
                        { autoReveal: !(settings?.autoReveal ?? true) },
                        () => api.setAutoReveal(!(settings?.autoReveal ?? true)),
                      )
                    }
                    aria-pressed={settings?.autoReveal ?? true}
                  >
                    <span />
                  </button>
                </label>
              </section>

              <section className="home-settings-section">
                <div className="home-section-title">
                  <ShieldCheck size={17} />
                  {t("accessCheck")}
                </div>
                <button className="settings-wide-button" onClick={() => void runAccessCheck()}>
                  {checking ? (
                    <LoaderCircle size={16} className="spin" />
                  ) : (
                    <RefreshCw size={16} />
                  )}
                  {checking ? t("checking") : t("checkAccess")}
                </button>
                {checks ? (
                  <div className="home-checks">
                    {checks.map((check) => (
                      <div
                        key={check.kind}
                        className={`home-check ${check.ok ? "is-ok" : "is-bad"}`}
                      >
                        {check.ok ? <Check size={15} /> : <X size={15} />}
                        <span>
                          <strong>{outputLabel(check.kind)}</strong>
                          <small>{check.ok ? t("accessOk") : t("accessDenied")}</small>
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
                      <ScanLine size={16} />
                      {t("screenPermission")}
                    </button>
                    <button onClick={() => void api.openFilesPermissionSettings()}>
                      <FolderOpen size={16} />
                      {t("filesPermission")}
                    </button>
                  </div>
                </section>
              ) : null}

              <section className="home-settings-section">
                <div className="home-section-title">{t("language")}</div>
                <div className="home-segment">
                  <button
                    className={lang === "en" ? "is-active" : ""}
                    onClick={() => void changeLanguage("en")}
                  >
                    English
                  </button>
                  <button
                    className={lang === "zh" ? "is-active" : ""}
                    onClick={() => void changeLanguage("zh")}
                  >
                    简体中文
                  </button>
                </div>
                {languageError ? (
                  <div className="runtime-error" role="alert">
                    <span>{languageError}</span>
                  </div>
                ) : null}
              </section>
            </div>
          ) : (
            <div
              className="home-settings-panel"
              id="settings-panel-runtime"
              role="tabpanel"
              aria-labelledby="settings-tab-runtime"
            >
              <section className="home-settings-section">
                <div className="runtime-section-head">
                  <div className="home-section-title">
                    <Cpu size={17} />
                    {t("runtimeComponents")}
                  </div>
                  <div className="runtime-head-actions">
                    {runtimeInstallable.length > 1 ? (
                      <button
                        className="runtime-install runtime-install-all"
                        onClick={() => void installRuntimeComponents(runtimeInstallable)}
                        disabled={runtimeLoading || runtimeInstalling.length > 0}
                      >
                        {runtimeInstalling.length > 0 ? (
                          <LoaderCircle size={14} className="spin" />
                        ) : (
                          <Download size={14} />
                        )}
                        {runtimeInstalling.length > 0
                          ? t("runtimeInstalling")
                          : t("runtimeInstallAll", { count: runtimeInstallable.length })}
                      </button>
                    ) : null}
                    <button
                      className="settings-link runtime-refresh"
                      onClick={() => void refreshRuntime()}
                      disabled={runtimeLoading || runtimeInstalling.length > 0}
                    >
                      <RefreshCw size={13} className={runtimeLoading ? "spin" : ""} />
                      {runtimeLoading ? t("runtimeChecking") : t("runtimeRefresh")}
                    </button>
                  </div>
                </div>
                <p className="runtime-summary">{t("runtimeSummary")}</p>
                {runtimeError ? (
                  <div className="runtime-error" role="alert">
                    <span>{runtimeError}</span>
                    {runtimeInstallHelp ? (
                      <button
                        className="runtime-error-link"
                        onClick={() =>
                          void openUrl(
                            runtimeInstallHelp === "homebrew"
                              ? "https://brew.sh/"
                              : "https://learn.microsoft.com/windows/package-manager/winget/",
                          )
                        }
                      >
                        {t(
                          runtimeInstallHelp === "homebrew"
                            ? "runtimeOpenHomebrew"
                            : "runtimeOpenWinget",
                        )}
                      </button>
                    ) : null}
                  </div>
                ) : null}
                {runtimeLoading && !runtimeInfo ? (
                  <div className="runtime-loading">
                    <LoaderCircle size={16} className="spin" />
                    {t("runtimeChecking")}
                  </div>
                ) : null}
                {runtimeGroups.map((group) => (
                  <div className="runtime-group" key={group.id}>
                    <div className="runtime-group-title">{group.label}</div>
                    {group.components.map((component) => (
                      <div
                        className={`runtime-component ${component.available ? "is-available" : "is-missing"}`}
                        key={component.id}
                      >
                        <div className="runtime-component-head">
                          <strong>{component.name}</strong>
                          <span className={`runtime-source is-${component.source}`}>
                            {t(`runtimeSource_${component.source}`)}
                          </span>
                        </div>
                        <div className="runtime-component-meta">
                          <span className="runtime-status">
                            {component.available ? <Check size={13} /> : <X size={13} />}
                            {component.available ? t("runtimeAvailable") : t("runtimeMissing")}
                          </span>
                          {component.version ? (
                            <span className="runtime-version" title={component.version}>
                              {component.version}
                            </span>
                          ) : null}
                        </div>
                        {component.path ? (
                          <code className="runtime-path" title={component.path}>
                            {component.path}
                          </code>
                        ) : component.source === "bundled" ? (
                          <span className="runtime-bundled">{t("runtimeBundledInApp")}</span>
                        ) : null}
                        {!component.available && component.installable ? (
                          <button
                            className="runtime-install"
                            onClick={() => void installRuntimeComponents([component])}
                            disabled={runtimeLoading || runtimeInstalling.length > 0}
                          >
                            {runtimeInstalling.includes(component.id) ? (
                              <LoaderCircle size={14} className="spin" />
                            ) : (
                              <Download size={14} />
                            )}
                            {runtimeInstalling.includes(component.id)
                              ? t("runtimeInstalling")
                              : t("runtimeInstall")}
                          </button>
                        ) : null}
                      </div>
                    ))}
                  </div>
                ))}
              </section>
            </div>
          )}
        </div>
      </aside>

      {sharePaths ? (
        <SharePanel
          paths={sharePaths}
          onClose={() => setSharePaths(null)}
          onNotice={(message) => flash(message, FEEDBACK.notice)}
        />
      ) : null}

      {toast ? (
        <div className="main-toast" role="status" aria-live="polite">
          {toast}
        </div>
      ) : null}
    </div>
  );
}
