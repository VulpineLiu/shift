import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
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
  RecordingCapabilities,
  RecordingInfo,
  RecordingOptions,
  SharePeer,
  RuntimeInfo,
  SwitchState,
  SwitchUpdate,
  UpdateProgress,
  UpdateStatus,
  WriteCheck,
} from "../types";
import type { WedgeOrders } from "./radialOrder";

interface FileActionResultEvent {
  jobId: string;
  outcome?: ActionOutcome;
  error?: string;
}

interface FileActionProgressEvent {
  jobId: string;
  done: number;
  total: number;
  packaging: boolean;
}

async function performFileAction(
  action: string,
  paths: string[],
  target?: string,
  onProgress?: (done: number, total: number, packaging: boolean) => void,
): Promise<ActionOutcome> {
  const jobId =
    typeof crypto.randomUUID === "function"
      ? crypto.randomUUID()
      : `${Date.now()}-${Math.random().toString(36).slice(2)}`;
  let resolveOutcome!: (outcome: ActionOutcome) => void;
  let rejectOutcome!: (reason?: unknown) => void;
  const completed = new Promise<ActionOutcome>((resolve, reject) => {
    resolveOutcome = resolve;
    rejectOutcome = reject;
  });
  const unlisten: UnlistenFn = await listen<FileActionResultEvent>(
    "app://file-action-finished",
    (event) => {
      if (event.payload.jobId !== jobId) return;
      if (event.payload.error) rejectOutcome(new Error(event.payload.error));
      else if (event.payload.outcome) resolveOutcome(event.payload.outcome);
      else rejectOutcome(new Error("errNoActionResult"));
    },
  );
  const unlistenProgress = onProgress
    ? await listen<FileActionProgressEvent>("app://file-action-progress", (event) => {
        if (event.payload.jobId === jobId) {
          onProgress(event.payload.done, event.payload.total, event.payload.packaging);
        }
      })
    : null;
  try {
    await invoke<void>("perform_file_action", { jobId, action, paths, target });
    return await completed;
  } finally {
    unlisten();
    unlistenProgress?.();
  }
}

export const api = {
  inspectFiles: (paths: string[]) => invoke<FileInfo[]>("inspect_files", { paths }),
  performFileAction,
  revealFile: (path: string) => invoke<void>("reveal_file", { path }),
  openFile: (path: string) => invoke<void>("open_file", { path }),
  readTextFile: (path: string) => invoke<string>("read_text_file", { path }),
  readImageData: (path: string) => invoke<string>("read_image_data", { path }),
  saveImageData: (source: string, dataUrl: string, suffix?: string) =>
    invoke<string>("save_image_data", { source, dataUrl, suffix }),
  renderPdfPages: (path: string) => invoke<string[]>("render_pdf_pages", { path }),
  cleanupPdfPages: (paths: string[]) => invoke<void>("cleanup_pdf_pages", { paths }),
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
  recordingCapabilities: () => invoke<RecordingCapabilities>("recording_capabilities"),
  stopRecording: () => invoke<RecordingInfo | null>("stop_recording"),
  recordingStatus: () => invoke<RecordingInfo | null>("recording_status"),
  getEditorSession: () => invoke<EditorInfo | null>("get_editor_session"),
  closeEditor: () => invoke<void>("close_editor"),
  openInEditor: (path: string) => invoke<EditorInfo>("open_in_editor", { path }),
  pinImage: (path: string) => invoke<void>("pin_image", { path }),
  getPinnedImage: () =>
    invoke<{ id: number; path: string; dataUrl: string; width: number; height: number } | null>(
      "get_pinned_image",
    ),
  pinReady: (id: number | null) => invoke<void>("pin_ready", { id }),
  closePin: () => invoke<void>("close_pin"),
  getRadialContext: () => invoke<RadialContext | null>("get_radial_context"),
  hideRadial: () => invoke<void>("hide_radial"),
  showFilesInRadial: (files: string[], mode?: "actions" | "formats" | "tools") =>
    invoke<RadialContext>("show_files_in_radial", { files, mode }),
  shareAnswer: (transferId: string, accepted: boolean) =>
    invoke<boolean>("share_answer", { transferId, accepted }),
  shareRefresh: () => invoke<SharePeer[]>("share_refresh"),
  shareSend: (peerId: string, paths: string[]) =>
    invoke<{ name: string; size: number; peer: string }[]>("share_send", { peerId, paths }),
  getSettings: () => invoke<AppSettings>("get_app_settings"),
  setOutputDirectory: (kind: "screenshot" | "recording" | "files", path: string | null) =>
    invoke<AppSettings>("set_output_directory", { kind, path }),
  setFileOutputMode: (mode: FileOutputMode) =>
    invoke<AppSettings>("set_file_output_mode", { mode }),
  setAutoReveal: (enabled: boolean) => invoke<SwitchState>("set_auto_reveal", { enabled }),
  setDragEnabled: (enabled: boolean) => invoke<SwitchState>("set_drag_enabled", { enabled }),
  setShareEnabled: (enabled: boolean) => invoke<SwitchState>("set_share_enabled", { enabled }),
  setAutostart: (enabled: boolean) => invoke<SwitchUpdate>("set_autostart", { enabled }),
  setLanguage: (language: string) => invoke<string>("set_language", { language }),
  hotkeyStatus: () => invoke<HotkeyStatus>("hotkey_status"),
  quitOtherInstances: () => invoke<number>("quit_other_instances"),
  reclaimHotkeys: () => invoke<HotkeyStatus>("reclaim_hotkeys"),
  checkOutputAccess: () => invoke<WriteCheck[]>("check_output_access"),
  revealOutputDirectory: (kind: "screenshot" | "recording" | "files") =>
    invoke<void>("reveal_output_directory", { kind }),
  openFilesPermissionSettings: () => invoke<void>("open_files_permission_settings"),
  setUpdateCheckEnabled: (enabled: boolean) =>
    invoke<SwitchState>("set_update_check_enabled", { enabled }),
  setSetupReviewed: (reviewed: boolean) => invoke<boolean>("set_setup_reviewed", { reviewed }),
  getWedgeOrders: () => invoke<WedgeOrders>("get_wedge_orders"),
  updateCheck: () =>
    invoke<{ status: UpdateStatus; downloadReady: boolean }>("update_check"),
  updateStatus: () => invoke<UpdateStatus | null>("update_status"),
  updateDownload: () => invoke<UpdateStatus>("update_download"),
  updateInstall: () => invoke<string>("update_install"),
  updateClear: () => invoke<void>("update_clear"),
  onUpdateProgress: (handler: (progress: UpdateProgress) => void) =>
    listen<UpdateProgress>("update://progress", (event) => handler(event.payload)),
  onUpdateStatus: (handler: (status: UpdateStatus | null) => void) =>
    listen<UpdateStatus | null>("update://status", (event) => handler(event.payload)),
  getRuntimeInfo: () => invoke<RuntimeInfo>("machine_info"),
  installRuntimeComponent: (componentId: string) =>
    invoke<string>("install_runtime_component", { componentId }),
};
