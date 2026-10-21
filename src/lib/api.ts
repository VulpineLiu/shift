import { invoke } from "@tauri-apps/api/core";
import type {
  ActionOutcome,
  AppSettings,
  CaptureSession,
  EditorInfo,
  HotkeyStatus,
  FileInfo,
  FileOutputMode,
  NormalizedRect,
  RadialContext,
  RecordingInfo,
  RecordingOptions,
  WriteCheck,
} from "../types";

export const api = {
  inspectFiles: (paths: string[]) => invoke<FileInfo[]>("inspect_files", { paths }),
  performFileAction: (action: string, paths: string[], target?: string) =>
    invoke<ActionOutcome>("perform_file_action", { action, paths, target }),
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  existingFiles: (paths: string[]) => invoke<string[]>("existing_files", { paths }),
  openFile: (path: string) => invoke<void>("open_file", { path }),
  readFileDataUrl: (path: string) => invoke<string>("read_file_data_url", { path }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  saveImageData: (source: string, dataUrl: string, suffix?: string) =>
    invoke<string>("save_image_data", { source, dataUrl, suffix }),
  renderPdfPages: (path: string) => invoke<string[]>("render_pdf_pages", { path }),
  saveTextNear: (source: string, text: string, suffix?: string) =>
    invoke<string>("save_text_near", { source, text, suffix }),
  copyFileTo: (source: string, destination: string) =>
    invoke<void>("copy_file_to", { source, destination }),
  openScreenRecordingSettings: () => invoke<void>("open_screen_recording_settings"),
  getCaptureSession: () => invoke<CaptureSession | null>("get_capture_session"),
  screenCaptureAccessStatus: () => invoke<boolean>("screen_capture_access_status"),
  startCapture: (mode: "screenshot" | "recording") =>
    invoke<CaptureSession>("start_capture", { mode }),
  cancelCapture: () => invoke<void>("cancel_capture"),
  captureCrop: (rect: NormalizedRect) => invoke<string>("capture_crop", { rect }),
  captureFullscreen: () => invoke<string>("capture_fullscreen"),
  startRecording: (rect: NormalizedRect, options?: RecordingOptions) =>
    invoke<RecordingInfo>("start_recording", { rect, options }),
  stopRecording: () => invoke<RecordingInfo | null>("stop_recording"),
  recordingStatus: () => invoke<RecordingInfo | null>("recording_status"),
  getEditorSession: () => invoke<EditorInfo | null>("get_editor_session"),
  closeEditor: () => invoke<void>("close_editor"),
  openInEditor: (path: string) => invoke<EditorInfo>("open_in_editor", { path }),
  pinImage: (path: string) => invoke<void>("pin_image", { path }),
  getRadialContext: () => invoke<RadialContext | null>("get_radial_context"),
  hideRadial: () => invoke<void>("hide_radial"),
  showFilesInRadial: (files: string[], mode?: "actions" | "formats" | "tools") =>
    invoke<RadialContext>("show_files_in_radial", { files, mode }),
  getSettings: () => invoke<AppSettings>("get_app_settings"),
  setOutputDirectory: (kind: "screenshot" | "recording" | "files", path: string | null) =>
    invoke<AppSettings>("set_output_directory", { kind, path }),
  setFileOutputMode: (mode: FileOutputMode) =>
    invoke<AppSettings>("set_file_output_mode", { mode }),
  setAutoReveal: (enabled: boolean) => invoke<AppSettings>("set_auto_reveal", { enabled }),
  setDragEnabled: (enabled: boolean) => invoke<AppSettings>("set_drag_enabled", { enabled }),
  setAutostart: (enabled: boolean) => invoke<AppSettings>("set_autostart", { enabled }),
  hotkeyStatus: () => invoke<HotkeyStatus>("hotkey_status"),
  quitOtherInstances: () => invoke<number>("quit_other_instances"),
  reclaimHotkeys: () => invoke<HotkeyStatus>("reclaim_hotkeys"),
  checkOutputAccess: () => invoke<WriteCheck[]>("check_output_access"),
  revealOutputDirectory: (kind: "screenshot" | "recording" | "files") =>
    invoke<void>("reveal_output_directory", { kind }),
  openFilesPermissionSettings: () => invoke<void>("open_files_permission_settings"),
};
