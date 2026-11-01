export type View =
  | "main"
  | "radial"
  | "capture"
  | "recorder"
  | "editor"
  | "pin"
  | "result-card"
  | "share-notice";

export interface FileInfo {
  path: string;
  name: string;
  extension: string;
  kind: "image" | "video" | "audio" | "pdf" | "document" | "presentation" | "spreadsheet" | "other";
  size: number;
  modified?: number;
  preview?: string;
  width?: number;
  height?: number;
}

export interface RadialContext {
  files: string[];
  fileInfo: FileInfo[];
  mode: "formats" | "tools" | "actions";
  dragging: boolean;
}

export interface CaptureSource {
  path: string;
  width: number;
  height: number;
  monitorX: number;
  monitorY: number;
  scaleFactor: number;
}

export interface CaptureSession {
  mode: "screenshot" | "recording";
  source: CaptureSource;
}

export interface ActionOutcome {
  outputs: string[];
  message: string;
}

export interface RecordingInfo {
  output: string;
  startedAt: number;
}

export interface EditorInfo {
  path: string;
  kind: "image" | "video" | "text" | "file";
}

export interface NormalizedRect {
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface RecordingOptions {
  audio: boolean;
  /** Omitted when the platform's capture tool decides the frame rate itself. */
  fps?: number;
}

/** Which recording switches the current platform actually honours. */
export interface RecordingCapabilities {
  audio: boolean;
  fps: boolean;
}

export type FileOutputMode = "alongside" | "custom";

export interface AppSettings {
  fileOutputMode: FileOutputMode;
  fileOutputDir: string;
  screenshotDir: string;
  recordingDir: string;
  autoReveal: boolean;
  dragEnabled: boolean;
  shareEnabled: boolean;
  autostart: boolean;
  /** "" until the user picks a language; the tray follows whatever is stored. */
  language: string;
  /** Ask the git release branch for a newer build once a day. */
  updateCheckEnabled: boolean;
  /** Raw URL of the release manifest this build reads; "" when unconfigured. */
  updateFeedUrl: string;
  /** Custom wedge order per kind and ring, keyed "<kind>/<ring>". */
  wedgeOrders: Record<string, string[]>;
  /** False until the guided first-run checks have been shown or dismissed. */
  setupReviewed: boolean;
  defaultFileOutputDir: string;
  defaultScreenshotDir: string;
  defaultRecordingDir: string;
}

export interface SwitchState {
  enabled: boolean;
}

export interface SwitchUpdate {
  state: SwitchState;
  autostart: boolean | null;
}

export interface HotkeyStatus {
  /** False when another copy owns the global hotkeys. */
  live: boolean;
  hotkeys: string[];
  /** Pids of other running Shift processes. */
  others: number[];
}

export interface WriteCheck {
  kind: "screenshot" | "recording" | "files";
  path: string;
  ok: boolean;
}

export type RuntimeComponentCategory = "documents" | "graphics" | "video" | "capture" | "ocr";

export interface RuntimeComponent {
  id: string;
  name: string;
  category: RuntimeComponentCategory;
  role: string;
  source: "bundled" | "system" | "external";
  available: boolean;
  installable: boolean;
  version?: string;
  path?: string;
}

export interface RuntimeInfo {
  platform: string;
  architecture: string;
  osVersion: string;
  appVersion: string;
  components: RuntimeComponent[];
}

/** Another Shift copy discovered on the local network. */
export interface SharePeer {
  id: string;
  name: string;
  platform: "macos" | "windows" | "linux" | string;
  address: string;
}

/** An incoming LAN file the user has not answered yet. */
export interface ShareOffer {
  transferId: string;
  fileName: string;
  size: number;
  peerName: string;
  timeoutSeconds: number;
}

export interface ShareTransfer {
  transferId: string;
  fileName: string;
  peerName: string;
  sent: number;
  total: number;
  state: "sending" | "received" | "error";
  path?: string | null;
}

/** Result of the git-backed update check, mirroring `update::UpdateStatus`. */
export interface UpdateStatus {
  state: "current" | "available" | "ready" | "unavailable" | "error";
  currentVersion: string;
  availableVersion: string;
  fileName: string;
  size: number;
  sha256: string;
  notes: string;
  date: string;
  /** Absolute path once the installer has been downloaded and hash-verified. */
  path: string | null;
  message: string;
  feedUrl: string;
  /** True when the manifest was signature-verified against a key baked into this
   * build. False means the feed is trusted through HTTPS alone. */
  signed: boolean;
  /** The manifest key this build resolved its artifact under, reported by Rust
   * rather than inferred, or "" when the release names nothing for it. */
  artifactKey: string;
}

export interface UpdateProgress {
  downloaded: number;
  total: number;
  state: "downloading" | "verifying";
}

/** One saved capture, handed to the result-card window. */
export interface CaptureResult {
  path: string;
  name: string;
  kind: "image" | "video";
  /** Milliseconds before the card hides itself. */
  ttlMs: number;
}
