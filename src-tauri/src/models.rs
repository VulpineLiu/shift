use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub path: String,
    pub name: String,
    pub extension: String,
    pub kind: String,
    pub size: u64,
    pub modified: Option<i64>,
    pub preview: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSource {
    pub path: String,
    pub width: u32,
    pub height: u32,
    pub monitor_x: f64,
    pub monitor_y: f64,
    pub scale_factor: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CaptureSessionInfo {
    pub mode: String,
    pub source: CaptureSource,
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NormalizedRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionOutcome {
    pub outputs: Vec<String>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionResultEvent {
    pub job_id: String,
    pub outcome: Option<ActionOutcome>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileActionProgress {
    pub job_id: String,
    pub done: u32,
    pub total: u32,
    pub packaging: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingInfo {
    pub output: String,
    pub started_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditorInfo {
    pub path: String,
    pub kind: String,
}

/// What the capture overlay asks for when a recording starts.
///
/// The two switches are honest: `recording_capabilities` says which of them the
/// current platform honours, and the overlay hides the rest, so a choice the backend
/// would ignore can never be presented as applied.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RecordingOptions {
    /// Record audio alongside video.
    pub audio: bool,
    /// Requested frame rate, where the backend accepts one. `None` leaves the choice
    /// to the capture tool instead of inventing a number.
    pub fps: Option<u32>,
}

/// Which recording switches this platform actually implements.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecordingCapabilities {
    /// Whether audio is captured with the video.
    pub audio: bool,
    /// Whether a frame-rate request is passed to the capture tool.
    pub fps: bool,
}
