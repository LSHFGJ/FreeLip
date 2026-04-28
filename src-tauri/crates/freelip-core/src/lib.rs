//! Core FreeLip ROI model-selection and quality-gate primitives.

use prost::Message;
use std::error::Error;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use tract_onnx::pb::ModelProto;
use tract_onnx::prelude::{tvec, Framework, InferenceModelExt};

const SCORE_ONLY_MAX_YAW_RATIO: f32 = 0.45;
const SCORE_ONLY_MAX_ROLL_DEGREES: f32 = 20.0;
const MILLIS_PER_DAY: u64 = 24 * 60 * 60 * 1_000;

pub const DEFAULT_DEBUG_LOG_RETENTION_DAYS: u64 = 7;
pub const DEFAULT_DEBUG_LOG_MAX_BYTES: u64 = 512 * 1024 * 1024;

/// Metadata for the selected Windows-local face/landmark ONNX model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoiLandmarkModelMetadata {
    pub model_id: &'static str,
    pub file_name: &'static str,
    pub source_url: &'static str,
    pub license: &'static str,
    pub sha256: &'static str,
    pub size_bytes: u64,
    pub landmark_layout: &'static str,
    pub runtime: &'static str,
}

/// OpenCV Zoo YuNet model selected for Task 3 ROI bootstrap.
pub const SELECTED_ROI_MODEL: RoiLandmarkModelMetadata = RoiLandmarkModelMetadata {
    model_id: "opencv-yunet-2023mar",
    file_name: "face_detection_yunet_2023mar.onnx",
    source_url: "https://github.com/opencv/opencv_zoo/blob/main/models/face_detection_yunet/face_detection_yunet_2023mar.onnx",
    license: "MIT",
    sha256: "8f2383e4dd3cfbb4553ea8718107fc0423210dc964f9f4280604804ed2552fa4",
    size_bytes: 232_589,
    landmark_layout: "bbox_xywh,right_eye,left_eye,nose_tip,right_mouth_corner,left_mouth_corner,confidence",
    runtime: "tract-onnx",
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugLogRetentionPolicy {
    pub max_age_days: u64,
    pub max_total_bytes: u64,
}

impl Default for DebugLogRetentionPolicy {
    fn default() -> Self {
        Self {
            max_age_days: DEFAULT_DEBUG_LOG_RETENTION_DAYS,
            max_total_bytes: DEFAULT_DEBUG_LOG_MAX_BYTES,
        }
    }
}

impl DebugLogRetentionPolicy {
    pub fn new(max_age_days: u64, max_total_bytes: u64) -> Result<Self, DebugLogRetentionError> {
        if max_age_days > DEFAULT_DEBUG_LOG_RETENTION_DAYS {
            return Err(DebugLogRetentionError::RetentionExceedsSevenDays {
                requested_days: max_age_days,
            });
        }
        Ok(Self {
            max_age_days,
            max_total_bytes,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugLogRetentionError {
    RetentionExceedsSevenDays { requested_days: u64 },
}

impl fmt::Display for DebugLogRetentionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RetentionExceedsSevenDays { requested_days } => write!(
                formatter,
                "debug log retention cannot exceed 7 days; requested {requested_days} days"
            ),
        }
    }
}

impl Error for DebugLogRetentionError {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLogFileRecord {
    pub path: PathBuf,
    pub modified_timestamp_ms: u64,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DebugLogRetentionPlan {
    pub expired_files: Vec<PathBuf>,
    pub size_cap_files: Vec<PathBuf>,
    pub retained_files: Vec<PathBuf>,
    pub retained_bytes: u64,
    pub bytes_to_remove: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoiDebugMetadata {
    pub request_id: String,
    pub session_id: String,
    pub created_timestamp_ms: u64,
    pub local_clip_path: Option<PathBuf>,
    pub quality_flags: Vec<String>,
    pub frame_count: u32,
    pub duration_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DebugCandidateSummary {
    pub rank: u8,
    pub text: String,
    pub score: f32,
    pub source: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertionOutcome {
    NotAttempted,
    AutoInserted,
    CandidateShown,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoOutcome {
    NotRequested,
    Undone,
    Expired,
    Failed,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DebugLogEvent {
    pub request_id: String,
    pub timestamp_ms: u64,
    pub quality_flags: Vec<String>,
    pub candidates: Vec<DebugCandidateSummary>,
    pub insertion_outcome: InsertionOutcome,
    pub undo_outcome: UndoOutcome,
    pub latency_ms: u64,
    pub model_id: String,
    pub failure_reason: Option<String>,
}

impl DebugLogEvent {
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }
}

pub fn default_debug_log_dir(app_data_root: impl AsRef<Path>) -> PathBuf {
    app_data_root.as_ref().join(".freelip").join("roi-debug")
}

pub fn ensure_debug_log_dir(app_data_root: impl AsRef<Path>) -> io::Result<PathBuf> {
    let debug_dir = default_debug_log_dir(app_data_root);
    fs::create_dir_all(&debug_dir)?;
    Ok(debug_dir)
}

pub fn plan_debug_log_retention(
    files: &[DebugLogFileRecord],
    now_timestamp_ms: u64,
    policy: DebugLogRetentionPolicy,
) -> DebugLogRetentionPlan {
    let max_age_ms = policy.max_age_days.saturating_mul(MILLIS_PER_DAY);
    let expiration_cutoff_ms = now_timestamp_ms.saturating_sub(max_age_ms);
    let mut expired = Vec::new();
    let mut retained = Vec::new();
    let mut bytes_to_remove = 0_u64;

    for file in files {
        if file.modified_timestamp_ms < expiration_cutoff_ms {
            expired.push(file.path.clone());
            bytes_to_remove = bytes_to_remove.saturating_add(file.size_bytes);
        } else {
            retained.push(file.clone());
        }
    }

    expired.sort();
    let mut retained_bytes = retained
        .iter()
        .fold(0_u64, |total, file| total.saturating_add(file.size_bytes));
    let mut size_cap_candidates = retained.clone();
    size_cap_candidates.sort_by(|left, right| {
        left.modified_timestamp_ms
            .cmp(&right.modified_timestamp_ms)
            .then_with(|| left.path.cmp(&right.path))
    });

    let mut size_cap_files = Vec::new();
    for file in size_cap_candidates {
        if retained_bytes <= policy.max_total_bytes {
            break;
        }
        retained_bytes = retained_bytes.saturating_sub(file.size_bytes);
        bytes_to_remove = bytes_to_remove.saturating_add(file.size_bytes);
        size_cap_files.push(file.path);
    }
    size_cap_files.sort();

    let mut retained_files: Vec<PathBuf> = retained
        .into_iter()
        .filter_map(|file| {
            if size_cap_files.contains(&file.path) {
                None
            } else {
                Some(file.path)
            }
        })
        .collect();
    retained_files.sort();

    DebugLogRetentionPlan {
        expired_files: expired,
        size_cap_files,
        retained_files,
        retained_bytes,
        bytes_to_remove,
    }
}

pub fn cleanup_debug_log_directory(
    debug_dir: impl AsRef<Path>,
    now: SystemTime,
    policy: DebugLogRetentionPolicy,
    dry_run: bool,
) -> io::Result<DebugLogRetentionPlan> {
    let debug_dir = debug_dir.as_ref();
    let files = collect_debug_log_files(debug_dir)?;
    let plan = plan_debug_log_retention(&files, system_time_to_unix_ms(now), policy);

    if !dry_run {
        for path in plan.expired_files.iter().chain(plan.size_cap_files.iter()) {
            match fs::remove_file(path) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error),
            }
        }
    }

    Ok(plan)
}

pub fn collect_debug_log_files(debug_dir: impl AsRef<Path>) -> io::Result<Vec<DebugLogFileRecord>> {
    let debug_dir = debug_dir.as_ref();
    let mut files = Vec::new();
    if !debug_dir.exists() {
        return Ok(files);
    }
    collect_debug_log_files_inner(debug_dir, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_debug_log_files_inner(
    directory: &Path,
    files: &mut Vec<DebugLogFileRecord>,
) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            collect_debug_log_files_inner(&entry.path(), files)?;
        } else if file_type.is_file() && is_debug_log_artifact(entry.path()) {
            let metadata = entry.metadata()?;
            let modified_timestamp_ms =
                metadata.modified().map(system_time_to_unix_ms).unwrap_or(0);
            files.push(DebugLogFileRecord {
                path: entry.path(),
                modified_timestamp_ms,
                size_bytes: metadata.len(),
            });
        }
    }
    Ok(())
}

pub fn is_debug_log_artifact(path: impl AsRef<Path>) -> bool {
    let Some(file_name) = path.as_ref().file_name().and_then(|name| name.to_str()) else {
        return false;
    };
    file_name.ends_with(".roi-debug.json")
        || file_name.ends_with(".roi-debug.log")
        || file_name.ends_with(".roi-debug.mp4")
}

fn system_time_to_unix_ms(value: SystemTime) -> u64 {
    value
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(u128::from(u64::MAX)) as u64)
        .unwrap_or(0)
}

/// Summary proving an ONNX model can be parsed and converted by the selected runtime.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelLoadSummary {
    pub runtime: &'static str,
    pub model_path: PathBuf,
    pub graph_name: String,
    pub input_count: usize,
    pub output_count: usize,
}

/// Model-loading errors kept explicit for later Task 7 diagnostics.
#[derive(Debug)]
pub enum RoiModelLoadError {
    ReadFailed {
        path: PathBuf,
        source: std::io::Error,
    },
    DecodeFailed {
        path: PathBuf,
        source: prost::DecodeError,
    },
    MissingGraph {
        path: PathBuf,
    },
    RuntimeRejected {
        path: PathBuf,
        source: String,
    },
    InferenceFailed {
        path: PathBuf,
        source: String,
    },
    MissingOutput {
        path: PathBuf,
    },
    OutputDecodeFailed {
        path: PathBuf,
        source: String,
    },
}

impl fmt::Display for RoiModelLoadError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ReadFailed { path, source } => {
                write!(
                    formatter,
                    "failed to read ONNX model {}: {source}",
                    path.display()
                )
            }
            Self::DecodeFailed { path, source } => {
                write!(
                    formatter,
                    "failed to decode ONNX model {}: {source}",
                    path.display()
                )
            }
            Self::MissingGraph { path } => {
                write!(
                    formatter,
                    "ONNX model {} does not contain a graph",
                    path.display()
                )
            }
            Self::RuntimeRejected { path, source } => write!(
                formatter,
                "ONNX runtime rejected model {}: {source}",
                path.display()
            ),
            Self::InferenceFailed { path, source } => write!(
                formatter,
                "ONNX runtime failed inference for model {}: {source}",
                path.display()
            ),
            Self::MissingOutput { path } => {
                write!(
                    formatter,
                    "ONNX model {} produced no outputs",
                    path.display()
                )
            }
            Self::OutputDecodeFailed { path, source } => write!(
                formatter,
                "failed to decode ONNX model {} f32 output: {source}",
                path.display()
            ),
        }
    }
}

impl Error for RoiModelLoadError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::ReadFailed { source, .. } => Some(source),
            Self::DecodeFailed { source, .. } => Some(source),
            Self::MissingGraph { .. }
            | Self::RuntimeRejected { .. }
            | Self::InferenceFailed { .. }
            | Self::MissingOutput { .. }
            | Self::OutputDecodeFailed { .. } => None,
        }
    }
}

/// Load an ONNX model through the selected runtime without retaining model bytes.
pub fn load_roi_landmark_model(
    path: impl AsRef<Path>,
) -> Result<ModelLoadSummary, RoiModelLoadError> {
    let path = path.as_ref();
    let model_path = path.to_path_buf();
    let bytes = fs::read(path).map_err(|source| RoiModelLoadError::ReadFailed {
        path: model_path.clone(),
        source,
    })?;
    let proto =
        ModelProto::decode(bytes.as_slice()).map_err(|source| RoiModelLoadError::DecodeFailed {
            path: model_path.clone(),
            source,
        })?;

    let graph = proto
        .graph
        .as_ref()
        .ok_or_else(|| RoiModelLoadError::MissingGraph {
            path: model_path.clone(),
        })?;
    let graph_name = graph.name.clone();
    let input_count = graph.input.len();
    let output_count = graph.output.len();

    tract_onnx::onnx()
        .model_for_proto_model(&proto)
        .and_then(|model| model.into_optimized())
        .map_err(|source| RoiModelLoadError::RuntimeRejected {
            path: model_path.clone(),
            source: source.to_string(),
        })?;

    Ok(ModelLoadSummary {
        runtime: SELECTED_ROI_MODEL.runtime,
        model_path,
        graph_name,
        input_count,
        output_count,
    })
}

/// Load and run a no-input ONNX fixture, returning the first f32 output.
pub fn run_no_input_onnx_f32_output(path: impl AsRef<Path>) -> Result<Vec<f32>, RoiModelLoadError> {
    let path = path.as_ref();
    let model_path = path.to_path_buf();
    let bytes = fs::read(path).map_err(|source| RoiModelLoadError::ReadFailed {
        path: model_path.clone(),
        source,
    })?;
    let proto =
        ModelProto::decode(bytes.as_slice()).map_err(|source| RoiModelLoadError::DecodeFailed {
            path: model_path.clone(),
            source,
        })?;

    proto
        .graph
        .as_ref()
        .ok_or_else(|| RoiModelLoadError::MissingGraph {
            path: model_path.clone(),
        })?;

    let runnable = tract_onnx::onnx()
        .model_for_proto_model(&proto)
        .and_then(|model| model.into_optimized())
        .and_then(|model| model.into_runnable())
        .map_err(|source| RoiModelLoadError::RuntimeRejected {
            path: model_path.clone(),
            source: source.to_string(),
        })?;

    let outputs = runnable
        .run(tvec![])
        .map_err(|source| RoiModelLoadError::InferenceFailed {
            path: model_path.clone(),
            source: source.to_string(),
        })?;
    let first = outputs
        .first()
        .ok_or_else(|| RoiModelLoadError::MissingOutput {
            path: model_path.clone(),
        })?;
    let values = first
        .to_array_view::<f32>()
        .map_err(|source| RoiModelLoadError::OutputDecodeFailed {
            path: model_path,
            source: source.to_string(),
        })?
        .iter()
        .copied()
        .collect();

    Ok(values)
}

/// Map a YuNet-style output row into FreeLip landmark fields.
pub fn face_detection_from_yunet_row(row: &[f32]) -> Option<FaceLandmarkDetection> {
    if row.len() < 15 || !row.iter().all(|value| value.is_finite()) {
        return None;
    }

    Some(FaceLandmarkDetection {
        face_bounds: Rect {
            x: row[0],
            y: row[1],
            width: row[2],
            height: row[3],
        },
        landmarks: FacialLandmarks {
            right_eye: Some(Point {
                x: row[4],
                y: row[5],
            }),
            left_eye: Some(Point {
                x: row[6],
                y: row[7],
            }),
            nose_tip: Some(Point {
                x: row[8],
                y: row[9],
            }),
            right_mouth_corner: Some(Point {
                x: row[10],
                y: row[11],
            }),
            left_mouth_corner: Some(Point {
                x: row[12],
                y: row[13],
            }),
        },
        confidence: row[14],
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

impl Point {
    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    fn is_finite(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }

    fn right(self) -> f32 {
        self.x + self.width
    }

    fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FacialLandmarks {
    pub right_eye: Option<Point>,
    pub left_eye: Option<Point>,
    pub nose_tip: Option<Point>,
    pub right_mouth_corner: Option<Point>,
    pub left_mouth_corner: Option<Point>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FaceLandmarkDetection {
    pub face_bounds: Rect,
    pub landmarks: FacialLandmarks,
    pub confidence: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FrameQuality {
    pub frame_width: u32,
    pub frame_height: u32,
    pub brightness: f32,
    pub blur_score: f32,
    pub face: Option<FaceLandmarkDetection>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiQualityThresholds {
    pub min_confidence: f32,
    pub min_brightness: f32,
    pub min_blur_score: f32,
    pub mouth_crop_width_scale: f32,
    pub mouth_crop_height_scale: f32,
    pub min_crop_width: f32,
    pub min_crop_height: f32,
}

impl Default for RoiQualityThresholds {
    fn default() -> Self {
        Self {
            min_confidence: 0.70,
            min_brightness: 0.30,
            min_blur_score: 0.45,
            mouth_crop_width_scale: 1.80,
            mouth_crop_height_scale: 1.20,
            min_crop_width: 32.0,
            min_crop_height: 24.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoiDecisionCode {
    RoiOk,
    NoFace,
    MouthOccluded,
    LowLight,
    Blurry,
    CropOutOfBounds,
}

impl RoiDecisionCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RoiOk => "ROI_OK",
            Self::NoFace => "NO_FACE",
            Self::MouthOccluded => "MOUTH_OCCLUDED",
            Self::LowLight => "LOW_LIGHT",
            Self::Blurry => "BLURRY",
            Self::CropOutOfBounds => "CROP_OUT_OF_BOUNDS",
        }
    }
}

impl fmt::Display for RoiDecisionCode {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoseApproximation {
    pub yaw_ratio: f32,
    pub roll_degrees: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiQualityReport {
    pub code: RoiDecisionCode,
    pub face_found: bool,
    pub mouth_landmarks_found: bool,
    pub crop_bounds_valid: bool,
    pub crop_bounds: Option<Rect>,
    pub blur_score: f32,
    pub brightness: f32,
    pub yaw_approx: f32,
    pub roll_degrees: f32,
    pub confidence: f32,
    pub quality_score: f32,
}

pub fn evaluate_roi_quality(
    frame: &FrameQuality,
    thresholds: &RoiQualityThresholds,
) -> RoiQualityReport {
    let Some(face) = frame.face else {
        return RoiQualityReport {
            code: RoiDecisionCode::NoFace,
            face_found: false,
            mouth_landmarks_found: false,
            crop_bounds_valid: false,
            crop_bounds: None,
            blur_score: frame.blur_score,
            brightness: frame.brightness,
            yaw_approx: 0.0,
            roll_degrees: 0.0,
            confidence: 0.0,
            quality_score: 0.0,
        };
    };

    let pose = yaw_roll_approximation(&face);
    let confidence_ok = confidence_passes(&face, thresholds);
    let mouth_found = mouth_landmarks_found(&face);
    let crop = mouth_crop_bounds(&face, thresholds);
    let crop_valid = crop
        .map(|bounds| crop_bounds_valid(bounds, frame, thresholds))
        .unwrap_or(false);
    let code = if !confidence_ok {
        RoiDecisionCode::NoFace
    } else if !mouth_found {
        RoiDecisionCode::MouthOccluded
    } else if !crop_valid {
        RoiDecisionCode::CropOutOfBounds
    } else if !brightness_passes(frame, thresholds) {
        RoiDecisionCode::LowLight
    } else if !blur_passes(frame, thresholds) {
        RoiDecisionCode::Blurry
    } else {
        RoiDecisionCode::RoiOk
    };

    RoiQualityReport {
        code,
        face_found: confidence_ok,
        mouth_landmarks_found: mouth_found,
        crop_bounds_valid: crop_valid,
        crop_bounds: crop.filter(|_| crop_valid && code == RoiDecisionCode::RoiOk),
        blur_score: frame.blur_score,
        brightness: frame.brightness,
        yaw_approx: pose.yaw_ratio,
        roll_degrees: pose.roll_degrees,
        confidence: face.confidence,
        quality_score: quality_score(frame, &face, thresholds, pose, crop_valid),
    }
}

pub fn confidence_passes(face: &FaceLandmarkDetection, thresholds: &RoiQualityThresholds) -> bool {
    face.confidence.is_finite() && face.confidence >= thresholds.min_confidence
}

pub fn mouth_landmarks_found(face: &FaceLandmarkDetection) -> bool {
    let right = face.landmarks.right_mouth_corner;
    let left = face.landmarks.left_mouth_corner;
    matches!((right, left), (Some(right), Some(left)) if right.is_finite() && left.is_finite() && distance(right, left) > 0.0)
}

pub fn mouth_crop_bounds(
    face: &FaceLandmarkDetection,
    thresholds: &RoiQualityThresholds,
) -> Option<Rect> {
    let right = face.landmarks.right_mouth_corner?;
    let left = face.landmarks.left_mouth_corner?;
    if !right.is_finite() || !left.is_finite() {
        return None;
    }

    let mouth_width = distance(right, left);
    if mouth_width <= 0.0 || !mouth_width.is_finite() {
        return None;
    }

    let center = Point {
        x: (right.x + left.x) / 2.0,
        y: (right.y + left.y) / 2.0,
    };
    let width = (mouth_width * thresholds.mouth_crop_width_scale).max(thresholds.min_crop_width);
    let height = (mouth_width * thresholds.mouth_crop_height_scale).max(thresholds.min_crop_height);

    Some(Rect {
        x: center.x - width / 2.0,
        y: center.y - height * 0.45,
        width,
        height,
    })
}

pub fn crop_bounds_valid(
    crop: Rect,
    frame: &FrameQuality,
    thresholds: &RoiQualityThresholds,
) -> bool {
    crop.is_finite()
        && crop.x >= 0.0
        && crop.y >= 0.0
        && crop.width >= thresholds.min_crop_width
        && crop.height >= thresholds.min_crop_height
        && crop.right() <= frame.frame_width as f32
        && crop.bottom() <= frame.frame_height as f32
}

pub fn brightness_passes(frame: &FrameQuality, thresholds: &RoiQualityThresholds) -> bool {
    frame.brightness.is_finite() && frame.brightness >= thresholds.min_brightness
}

pub fn blur_passes(frame: &FrameQuality, thresholds: &RoiQualityThresholds) -> bool {
    frame.blur_score.is_finite() && frame.blur_score >= thresholds.min_blur_score
}

pub fn yaw_roll_approximation(face: &FaceLandmarkDetection) -> PoseApproximation {
    let (Some(right_eye), Some(left_eye), Some(nose_tip)) = (
        face.landmarks.right_eye,
        face.landmarks.left_eye,
        face.landmarks.nose_tip,
    ) else {
        return PoseApproximation {
            yaw_ratio: 0.0,
            roll_degrees: 0.0,
        };
    };

    let eye_mid_x = (right_eye.x + left_eye.x) / 2.0;
    let yaw_ratio = if face.face_bounds.width > 0.0 {
        (nose_tip.x - eye_mid_x) / face.face_bounds.width
    } else {
        0.0
    };
    let roll_degrees = (left_eye.y - right_eye.y)
        .atan2(left_eye.x - right_eye.x)
        .to_degrees();

    PoseApproximation {
        yaw_ratio,
        roll_degrees,
    }
}

fn quality_score(
    frame: &FrameQuality,
    face: &FaceLandmarkDetection,
    thresholds: &RoiQualityThresholds,
    pose: PoseApproximation,
    crop_valid: bool,
) -> f32 {
    let confidence = normalized(face.confidence, thresholds.min_confidence);
    let brightness = normalized(frame.brightness, thresholds.min_brightness);
    let blur = normalized(frame.blur_score, thresholds.min_blur_score);
    let yaw = 1.0 - normalized_abs(pose.yaw_ratio, SCORE_ONLY_MAX_YAW_RATIO);
    let roll = 1.0 - normalized_abs(pose.roll_degrees, SCORE_ONLY_MAX_ROLL_DEGREES);
    let crop = if crop_valid { 1.0 } else { 0.0 };

    ((confidence + brightness + blur + yaw + roll + crop) / 6.0).clamp(0.0, 1.0)
}

fn normalized(value: f32, threshold: f32) -> f32 {
    if threshold <= 0.0 || !value.is_finite() {
        return 0.0;
    }
    (value / threshold).clamp(0.0, 1.0)
}

fn normalized_abs(value: f32, threshold: f32) -> f32 {
    if threshold <= 0.0 || !value.is_finite() {
        return 1.0;
    }
    (value.abs() / threshold).clamp(0.0, 1.0)
}

fn distance(a: Point, b: Point) -> f32 {
    let dx = a.x - b.x;
    let dy = a.y - b.y;
    (dx * dx + dy * dy).sqrt()
}
