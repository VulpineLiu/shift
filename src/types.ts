export type View = "main" | "radial" | "capture" | "recorder" | "editor" | "pin";

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
  fps: number;
}

export type FileOutputMode = "alongside" | "custom";

export interface AppSettings {
  fileOutputMode: FileOutputMode;
  fileOutputDir: string;
  screenshotDir: string;
  recordingDir: string;
  autoReveal: boolean;
  dragEnabled: boolean;
  autostart: boolean;
  defaultFileOutputDir: string;
  defaultScreenshotDir: string;
  defaultRecordingDir: string;
}

export interface HotkeyStatus {
  /** False when another copy owns the global hotkeys. */
  live: boolean;
  hotkeys: string[];
  /** Pids of other running Shift processes. */
  others: number[];
  /** A second copy tried to start in the last ten minutes. */
  recentDuplicate: boolean;
}

export interface WriteCheck {
  kind: "screenshot" | "recording" | "files";
  path: string;
  ok: boolean;
  message: string;
}
