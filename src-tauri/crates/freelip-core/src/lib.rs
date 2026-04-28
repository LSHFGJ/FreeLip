//! Core FreeLip ROI model-selection and quality-gate primitives.

use prost::Message;
use std::collections::BTreeMap;
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
const DICTIONARY_SCHEMA_VERSION: &str = "1.0.0";
const DEFAULT_DICTIONARY_WEIGHT: f32 = 0.50;
const MANUAL_DICTIONARY_DELTA: f32 = 0.30;
const AUTO_INSERT_DICTIONARY_DELTA: f32 = 0.10;
const UNDO_DICTIONARY_DELTA: f32 = -0.30;
const DICTIONARY_RANK_BOOST_SCALE: f32 = 0.20;
const MAX_LOCAL_RERANK_CANDIDATES: usize = 5;

pub const DEFAULT_DEBUG_LOG_RETENTION_DAYS: u64 = 7;
pub const DEFAULT_DEBUG_LOG_MAX_BYTES: u64 = 512 * 1024 * 1024;
pub const DEFAULT_NORMALIZED_ROI_WIDTH: u32 = 96;
pub const DEFAULT_NORMALIZED_ROI_HEIGHT: u32 = 96;
pub const CNVSRC_CENTER_CROP_SIZE: u32 = 88;
pub const DEFAULT_ROI_FPS: f32 = 25.0;
pub const CNVSRC_COMPATIBILITY_NOTE: &str =
    "96x96 grayscale_u8; center-crop to 88x88 before CNVSRC normalization mean=0.421 std=0.165";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DictionaryLearningSignal {
    ManualSelection,
    ManualCorrection,
    AutoInsertNotUndone,
    UndoWithinThreeSeconds,
}

impl DictionaryLearningSignal {
    pub fn weight_delta(self) -> f32 {
        match self {
            Self::ManualSelection | Self::ManualCorrection => MANUAL_DICTIONARY_DELTA,
            Self::AutoInsertNotUndone => AUTO_INSERT_DICTIONARY_DELTA,
            Self::UndoWithinThreeSeconds => UNDO_DICTIONARY_DELTA,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryEntry {
    pub schema_version: String,
    pub entry_id: String,
    pub surface: String,
    pub reading: Option<String>,
    pub weight: f32,
    pub tags: Vec<String>,
    pub updated_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct DictionaryTerm {
    pub surface: String,
    pub weight: f32,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct PersonalDictionary {
    entries: BTreeMap<String, DictionaryEntry>,
}

impl PersonalDictionary {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn learn(
        &mut self,
        surface: &str,
        signal: DictionaryLearningSignal,
        updated_at_ms: u64,
    ) -> DictionaryEntry {
        self.learn_with_metadata(surface, None, &[], signal, updated_at_ms)
    }

    pub fn learn_with_metadata(
        &mut self,
        surface: &str,
        reading: Option<&str>,
        tags: &[&str],
        signal: DictionaryLearningSignal,
        updated_at_ms: u64,
    ) -> DictionaryEntry {
        let surface = normalized_dictionary_text(surface);
        let entry_id = dictionary_entry_id(&surface);
        let entry = self
            .entries
            .entry(entry_id.clone())
            .or_insert_with(|| DictionaryEntry {
                schema_version: DICTIONARY_SCHEMA_VERSION.to_string(),
                entry_id,
                surface: surface.clone(),
                reading: None,
                weight: DEFAULT_DICTIONARY_WEIGHT,
                tags: Vec::new(),
                updated_at_ms,
            });

        entry.surface = surface;
        if let Some(reading) = reading.and_then(non_empty_dictionary_text) {
            entry.reading = Some(reading);
        }
        if !tags.is_empty() {
            entry.tags = normalized_tags(tags);
        }
        entry.weight = (entry.weight + signal.weight_delta()).clamp(0.0, 1.0);
        entry.updated_at_ms = updated_at_ms;
        entry.clone()
    }

    pub fn export_entries(&self) -> Vec<DictionaryEntry> {
        self.entries.values().cloned().collect()
    }

    pub fn dictionary_terms(&self) -> Vec<DictionaryTerm> {
        self.entries
            .values()
            .map(|entry| DictionaryTerm {
                surface: entry.surface.clone(),
                weight: entry.weight.clamp(0.0, 1.0),
                tags: entry.tags.clone(),
            })
            .collect()
    }

    pub fn delete_entry(&mut self, entry_id: &str) -> Option<DictionaryEntry> {
        self.entries.remove(entry_id)
    }

    pub fn clear(&mut self) -> usize {
        let removed_count = self.entries.len();
        self.entries.clear();
        removed_count
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocalRankCandidate {
    pub schema_version: String,
    pub rank: u8,
    pub text: String,
    pub score: f32,
    pub source: String,
    pub is_auto_insert_eligible: bool,
}

impl LocalRankCandidate {
    pub fn new(
        rank: u8,
        text: &str,
        score: f32,
        source: &str,
        is_auto_insert_eligible: bool,
    ) -> Self {
        Self {
            schema_version: DICTIONARY_SCHEMA_VERSION.to_string(),
            rank,
            text: text.to_string(),
            score: score.clamp(0.0, 1.0),
            source: allowed_candidate_source(source).to_string(),
            is_auto_insert_eligible,
        }
    }
}

pub fn rank_candidates_locally(
    candidates: &[LocalRankCandidate],
    dictionary_terms: &[DictionaryTerm],
    max_candidates: usize,
) -> Vec<LocalRankCandidate> {
    let limit = max_candidates.clamp(1, MAX_LOCAL_RERANK_CANDIDATES);
    let mut scored: Vec<(usize, LocalRankCandidate)> = candidates
        .iter()
        .cloned()
        .enumerate()
        .map(|(index, mut candidate)| {
            candidate.score = (candidate.score
                + dictionary_boost(&candidate.text, dictionary_terms))
            .clamp(0.0, 1.0);
            (index, candidate)
        })
        .collect();

    scored.sort_by(|(left_index, left), (right_index, right)| {
        right
            .score
            .total_cmp(&left.score)
            .then_with(|| left_index.cmp(right_index))
    });

    scored
        .into_iter()
        .take(limit)
        .enumerate()
        .map(|(index, (_, mut candidate))| {
            candidate.rank = (index + 1) as u8;
            candidate
        })
        .collect()
}

fn dictionary_boost(candidate_text: &str, dictionary_terms: &[DictionaryTerm]) -> f32 {
    dictionary_terms
        .iter()
        .filter(|term| !term.surface.is_empty() && candidate_text.contains(&term.surface))
        .map(|term| term.weight.clamp(0.0, 1.0) * DICTIONARY_RANK_BOOST_SCALE)
        .sum::<f32>()
        .clamp(0.0, 1.0)
}

fn dictionary_entry_id(surface: &str) -> String {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in surface.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("dict-{hash:016x}")
}

fn normalized_dictionary_text(value: &str) -> String {
    value.trim().chars().take(128).collect()
}

fn non_empty_dictionary_text(value: &str) -> Option<String> {
    let value = normalized_dictionary_text(value);
    if value.is_empty() {
        None
    } else {
        Some(value)
    }
}

fn normalized_tags(tags: &[&str]) -> Vec<String> {
    let mut normalized = Vec::new();
    for tag in tags {
        let tag: String = tag.trim().chars().take(64).collect();
        if !tag.is_empty() && !normalized.contains(&tag) {
            normalized.push(tag);
        }
        if normalized.len() == 16 {
            break;
        }
    }
    normalized
}

fn allowed_candidate_source(source: &str) -> &'static str {
    match source {
        "vsr" => "vsr",
        "cnvsrc2025" => "cnvsrc2025",
        "dictionary" => "dictionary",
        "llm_rerank" => "llm_rerank",
        "manual" => "manual",
        _ => "vsr",
    }
}

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

pub const INSERTION_SCHEMA_VERSION: &str = "1.0.0";
pub const DEFAULT_UNDO_WINDOW_MS: u64 = 3_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetApp {
    pub process_name: String,
    pub window_title_hash: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextContext {
    pub target_app: TargetApp,
    pub current_text: String,
    pub selection_start: usize,
    pub selection_end: usize,
    pub is_secure_field: bool,
    pub is_elevated: bool,
    pub supports_ctrl_z: bool,
    pub ctrl_z_safe_for_last_insert: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiaFieldSnapshot {
    pub target_app: TargetApp,
    pub text: String,
    pub is_password: bool,
    pub is_secure_field: bool,
    pub is_elevated: bool,
    pub supports_ctrl_z: bool,
    pub ctrl_z_safe_for_last_insert: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UiaSkipReason {
    SecureFieldSkipped,
    ElevatedAppSkipped,
}

impl UiaSkipReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SecureFieldSkipped => "SECURE_FIELD_SKIPPED",
            Self::ElevatedAppSkipped => "ELEVATED_APP_SKIPPED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UiaContextDecision {
    pub context: Option<TextContext>,
    pub skip_reason: Option<UiaSkipReason>,
}

impl UiaContextDecision {
    pub fn reason_code(&self) -> Option<&'static str> {
        self.skip_reason.map(UiaSkipReason::as_str)
    }
}

pub fn process_uia_context(snapshot: UiaFieldSnapshot) -> UiaContextDecision {
    if snapshot.is_password || snapshot.is_secure_field {
        return UiaContextDecision {
            context: None,
            skip_reason: Some(UiaSkipReason::SecureFieldSkipped),
        };
    }
    if snapshot.is_elevated {
        return UiaContextDecision {
            context: None,
            skip_reason: Some(UiaSkipReason::ElevatedAppSkipped),
        };
    }

    let cursor = snapshot.text.chars().count();
    UiaContextDecision {
        context: Some(TextContext {
            target_app: snapshot.target_app,
            current_text: snapshot.text,
            selection_start: cursor,
            selection_end: cursor,
            is_secure_field: false,
            is_elevated: false,
            supports_ctrl_z: snapshot.supports_ctrl_z,
            ctrl_z_safe_for_last_insert: snapshot.ctrl_z_safe_for_last_insert,
        }),
        skip_reason: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OperationAttempt {
    pub attempted: bool,
    pub succeeded: bool,
}

impl OperationAttempt {
    pub fn succeeded() -> Self {
        Self {
            attempted: true,
            succeeded: true,
        }
    }

    pub fn failed() -> Self {
        Self {
            attempted: true,
            succeeded: false,
        }
    }

    pub fn not_attempted() -> Self {
        Self {
            attempted: false,
            succeeded: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertionMethod {
    ClipboardPaste,
    SendInput,
    ManualSelection,
}

impl InsertionMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ClipboardPaste => "clipboard_paste",
            Self::SendInput => "send_input",
            Self::ManualSelection => "manual_selection",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertRecordStatus {
    Inserted,
    Undone,
    Failed,
}

impl InsertRecordStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Inserted => "inserted",
            Self::Undone => "undone",
            Self::Failed => "failed",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertRecord {
    pub schema_version: String,
    pub insert_id: String,
    pub session_id: String,
    pub candidate_text: String,
    pub target_app: TargetApp,
    pub method: InsertionMethod,
    pub inserted_at_ms: u64,
    pub undo_expires_at_ms: u64,
    pub status: InsertRecordStatus,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ConfirmedInsertionState {
    record: InsertRecord,
    expected_text_after_insert: String,
    insertion_start: usize,
    insertion_end: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmedInsertRequest {
    pub insert_id: String,
    pub session_id: String,
    pub candidate_text: String,
    pub target_app: TargetApp,
    pub inserted_at_ms: u64,
    pub context_before_insert: TextContext,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InsertionExecutionReport {
    pub clipboard_saved: bool,
    pub ctrl_v: OperationAttempt,
    pub send_input: OperationAttempt,
    pub clipboard_restore: OperationAttempt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InsertionFailureReason {
    EmptyCandidate,
    SecureFieldSkipped,
    ElevatedAppSkipped,
    ClipboardSaveFailed,
    CtrlVNotAttempted,
    InsertFailed,
    ClipboardRestoreNotAttempted,
}

impl InsertionFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EmptyCandidate => "EMPTY_CANDIDATE",
            Self::SecureFieldSkipped => "SECURE_FIELD_SKIPPED",
            Self::ElevatedAppSkipped => "ELEVATED_APP_SKIPPED",
            Self::ClipboardSaveFailed => "CLIPBOARD_SAVE_FAILED",
            Self::CtrlVNotAttempted => "CTRL_V_NOT_ATTEMPTED",
            Self::InsertFailed => "INSERT_FAILED",
            Self::ClipboardRestoreNotAttempted => "CLIPBOARD_RESTORE_NOT_ATTEMPTED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InsertionDecision {
    pub confirmed: bool,
    pub record: Option<InsertRecord>,
    pub failure_reason: Option<InsertionFailureReason>,
    pub clipboard_restore_attempted: bool,
    pub clipboard_restored: bool,
}

impl InsertionDecision {
    pub fn reason_code(&self) -> Option<&'static str> {
        self.failure_reason.map(InsertionFailureReason::as_str)
    }
}

pub fn clipboard_restore_required_after_insert(decision: &InsertionDecision) -> bool {
    decision.failure_reason == Some(InsertionFailureReason::ClipboardRestoreNotAttempted)
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct InsertionStateMachine {
    last_insert: Option<ConfirmedInsertionState>,
}

impl InsertionStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn last_insert(&self) -> Option<&InsertRecord> {
        self.last_insert.as_ref().map(|state| &state.record)
    }

    pub fn confirm_insertion(
        &mut self,
        request: ConfirmedInsertRequest,
        report: InsertionExecutionReport,
    ) -> InsertionDecision {
        let failure = insertion_failure(&request, &report);
        if let Some(failure_reason) = failure {
            return InsertionDecision {
                confirmed: false,
                record: None,
                failure_reason: Some(failure_reason),
                clipboard_restore_attempted: report.clipboard_restore.attempted,
                clipboard_restored: report.clipboard_restore.succeeded,
            };
        }

        let method = if report.ctrl_v.succeeded {
            InsertionMethod::ClipboardPaste
        } else {
            InsertionMethod::SendInput
        };
        let insertion_start = request
            .context_before_insert
            .selection_start
            .min(request.context_before_insert.current_text.chars().count());
        let insertion_end = insertion_start + request.candidate_text.chars().count();
        let expected_text_after_insert =
            text_after_insert(&request.context_before_insert, &request.candidate_text);
        let record = InsertRecord {
            schema_version: INSERTION_SCHEMA_VERSION.to_string(),
            insert_id: request.insert_id,
            session_id: request.session_id,
            candidate_text: request.candidate_text.clone(),
            target_app: request.target_app,
            method,
            inserted_at_ms: request.inserted_at_ms,
            undo_expires_at_ms: request
                .inserted_at_ms
                .saturating_add(DEFAULT_UNDO_WINDOW_MS),
            status: InsertRecordStatus::Inserted,
        };
        self.last_insert = Some(ConfirmedInsertionState {
            record: record.clone(),
            expected_text_after_insert,
            insertion_start,
            insertion_end,
        });

        InsertionDecision {
            confirmed: true,
            record: Some(record),
            failure_reason: None,
            clipboard_restore_attempted: report.clipboard_restore.attempted,
            clipboard_restored: report.clipboard_restore.succeeded,
        }
    }

    pub fn plan_undo(&self, context: TextContext, now_ms: u64) -> UndoPlan {
        let Some(state) = self.last_insert.as_ref() else {
            return UndoPlan::blocked(UndoReason::NoConfirmedInsertion);
        };
        let record = &state.record;

        if context.is_secure_field {
            return UndoPlan::blocked(UndoReason::SecureFieldSkipped);
        }
        if context.is_elevated {
            return UndoPlan::blocked(UndoReason::ElevatedAppSkipped);
        }
        if now_ms > record.undo_expires_at_ms {
            return UndoPlan::blocked(UndoReason::UndoExpired);
        }
        if context.target_app != record.target_app {
            return UndoPlan::blocked(UndoReason::FocusChanged);
        }
        if context.current_text == state.expected_text_after_insert {
            return UndoPlan {
                allowed: true,
                action: UndoAction::DeleteInsertedText,
                reason: None,
                delete_start: Some(state.insertion_start),
                delete_end: Some(state.insertion_end),
                inserted_text: Some(record.candidate_text.clone()),
            };
        }
        if context.supports_ctrl_z && context.ctrl_z_safe_for_last_insert {
            return UndoPlan {
                allowed: true,
                action: UndoAction::SendCtrlZ,
                reason: None,
                delete_start: None,
                delete_end: None,
                inserted_text: Some(record.candidate_text.clone()),
            };
        }

        UndoPlan::blocked(UndoReason::UserTypedAfterInsert)
    }

    pub fn finish_undo(&mut self, plan: UndoPlan, report: UndoExecutionReport) -> UndoResult {
        if !plan.allowed {
            return UndoResult {
                undone: false,
                action: plan.action,
                reason: plan.reason,
                clipboard_restore_attempted: report.clipboard_restore.attempted,
                clipboard_restored: report.clipboard_restore.succeeded,
                record: None,
            };
        }

        let destructive_action_succeeded = report.destructive_action.succeeded;
        let restore_succeeded = report.clipboard_restore.succeeded;
        let undone_record = if destructive_action_succeeded && restore_succeeded {
            self.last_insert.take().map(|mut state| {
                state.record.status = InsertRecordStatus::Undone;
                state.record
            })
        } else {
            None
        };
        let undone = undone_record.is_some();

        UndoResult {
            undone,
            action: plan.action,
            reason: if undone {
                None
            } else {
                Some(UndoReason::UndoExecutionFailed)
            },
            clipboard_restore_attempted: report.clipboard_restore.attempted,
            clipboard_restored: report.clipboard_restore.succeeded,
            record: undone_record,
        }
    }
}

fn insertion_failure(
    request: &ConfirmedInsertRequest,
    report: &InsertionExecutionReport,
) -> Option<InsertionFailureReason> {
    if request.candidate_text.trim().is_empty() {
        return Some(InsertionFailureReason::EmptyCandidate);
    }
    if request.context_before_insert.is_secure_field {
        return Some(InsertionFailureReason::SecureFieldSkipped);
    }
    if request.context_before_insert.is_elevated {
        return Some(InsertionFailureReason::ElevatedAppSkipped);
    }
    if !report.clipboard_saved {
        return Some(InsertionFailureReason::ClipboardSaveFailed);
    }
    if !report.ctrl_v.attempted {
        return Some(InsertionFailureReason::CtrlVNotAttempted);
    }
    if !report.ctrl_v.succeeded && !report.send_input.succeeded {
        return Some(InsertionFailureReason::InsertFailed);
    }
    if !report.clipboard_restore.attempted {
        return Some(InsertionFailureReason::ClipboardRestoreNotAttempted);
    }
    None
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoAction {
    Blocked,
    DeleteInsertedText,
    SendCtrlZ,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UndoReason {
    NoConfirmedInsertion,
    UndoExpired,
    FocusChanged,
    UserTypedAfterInsert,
    SecureFieldSkipped,
    ElevatedAppSkipped,
    UndoExecutionFailed,
}

impl UndoReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NoConfirmedInsertion => "NO_CONFIRMED_INSERTION",
            Self::UndoExpired => "UNDO_EXPIRED",
            Self::FocusChanged => "FOCUS_CHANGED",
            Self::UserTypedAfterInsert => "USER_TYPED_AFTER_INSERT",
            Self::SecureFieldSkipped => "SECURE_FIELD_SKIPPED",
            Self::ElevatedAppSkipped => "ELEVATED_APP_SKIPPED",
            Self::UndoExecutionFailed => "UNDO_EXECUTION_FAILED",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoPlan {
    pub allowed: bool,
    pub action: UndoAction,
    pub reason: Option<UndoReason>,
    pub delete_start: Option<usize>,
    pub delete_end: Option<usize>,
    pub inserted_text: Option<String>,
}

impl UndoPlan {
    fn blocked(reason: UndoReason) -> Self {
        Self {
            allowed: false,
            action: UndoAction::Blocked,
            reason: Some(reason),
            delete_start: None,
            delete_end: None,
            inserted_text: None,
        }
    }

    pub fn reason_code(&self) -> Option<&'static str> {
        self.reason.map(UndoReason::as_str)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UndoExecutionReport {
    pub destructive_action: OperationAttempt,
    pub clipboard_restore: OperationAttempt,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UndoResult {
    pub undone: bool,
    pub action: UndoAction,
    pub reason: Option<UndoReason>,
    pub clipboard_restore_attempted: bool,
    pub clipboard_restored: bool,
    pub record: Option<InsertRecord>,
}

impl UndoResult {
    pub fn reason_code(&self) -> Option<&'static str> {
        self.reason.map(UndoReason::as_str)
    }
}

fn text_after_insert(context: &TextContext, candidate_text: &str) -> String {
    let total_chars = context.current_text.chars().count();
    let start = context.selection_start.min(total_chars);
    let end = context.selection_end.min(total_chars).max(start);
    let start_byte = char_to_byte_index(&context.current_text, start);
    let end_byte = char_to_byte_index(&context.current_text, end);
    let mut value = String::with_capacity(context.current_text.len() + candidate_text.len());
    value.push_str(&context.current_text[..start_byte]);
    value.push_str(candidate_text);
    value.push_str(&context.current_text[end_byte..]);
    value
}

fn char_to_byte_index(value: &str, char_index: usize) -> usize {
    value
        .char_indices()
        .nth(char_index)
        .map(|(byte_index, _)| byte_index)
        .unwrap_or(value.len())
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

#[derive(Debug, Clone, PartialEq)]
pub struct RoiPipelineConfig {
    pub thresholds: RoiQualityThresholds,
    pub target_width: u32,
    pub target_height: u32,
    pub fps: f32,
    pub smoothing_alpha: f32,
    pub cnvsrc_center_crop_size: u32,
}

impl Default for RoiPipelineConfig {
    fn default() -> Self {
        Self {
            thresholds: RoiQualityThresholds::default(),
            target_width: DEFAULT_NORMALIZED_ROI_WIDTH,
            target_height: DEFAULT_NORMALIZED_ROI_HEIGHT,
            fps: DEFAULT_ROI_FPS,
            smoothing_alpha: 0.50,
            cnvsrc_center_crop_size: CNVSRC_CENTER_CROP_SIZE,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoiPipelineFrame {
    pub request_id: String,
    pub session_id: String,
    pub source_kind: String,
    pub device_id_hash: Option<String>,
    pub source_started_at_ms: u64,
    pub requested_at_ms: u64,
    pub frame_count: u32,
    pub duration_ms: u64,
    pub local_ref: String,
    pub quality: FrameQuality,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoiPipelineDecision {
    pub code: RoiDecisionCode,
    pub user_prompt_code: &'static str,
    pub quality_flags: RoiQualityFlags,
    pub raw_crop_bounds: Option<Rect>,
    pub smoothed_crop_bounds: Option<Rect>,
    pub roi_request: Option<RoiRequestMetadata>,
    pub should_emit_sidecar_decode: bool,
    pub sidecar_decode_requests: u32,
    pub frame_summary: RoiFrameSummary,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoiRequestMetadata {
    pub schema_version: String,
    pub request_id: String,
    pub session_id: String,
    pub source: RoiRequestSource,
    pub roi: NormalizedRoiClip,
    pub quality_flags: RoiQualityFlags,
    pub requested_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoiRequestSource {
    pub kind: String,
    pub device_id_hash: Option<String>,
    pub started_at_ms: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct NormalizedRoiClip {
    pub local_ref: String,
    pub format: String,
    pub width: u32,
    pub height: u32,
    pub fps: f32,
    pub frame_count: u32,
    pub duration_ms: u64,
    pub cnvsrc_center_crop_size: u32,
    pub cnvsrc_compatibility_note: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RoiQualityFlags {
    pub schema_version: String,
    pub face_found: bool,
    pub mouth_landmarks_found: bool,
    pub crop_bounds_valid: bool,
    pub blur_ok: bool,
    pub brightness_ok: bool,
    pub pose_ok: bool,
    pub occlusion_ok: bool,
    pub landmark_confidence: f32,
    pub rejection_reasons: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RoiFrameSummary {
    pub observed_frames: u32,
    pub accepted_frames: u32,
    pub rejected_frames: u32,
    pub sidecar_decode_requests: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoiJitterSmoother {
    alpha: f32,
    previous: Option<Rect>,
}

impl RoiJitterSmoother {
    pub fn new(alpha: f32) -> Self {
        Self {
            alpha: alpha.clamp(0.0, 1.0),
            previous: None,
        }
    }

    pub fn smooth(&mut self, crop: Rect) -> Rect {
        let smoothed = match self.previous {
            Some(previous) => Rect {
                x: exponential_average(previous.x, crop.x, self.alpha),
                y: exponential_average(previous.y, crop.y, self.alpha),
                width: exponential_average(previous.width, crop.width, self.alpha),
                height: exponential_average(previous.height, crop.height, self.alpha),
            },
            None => crop,
        };
        self.previous = Some(smoothed);
        smoothed
    }
}

pub fn process_roi_frame(
    frame: RoiPipelineFrame,
    config: &RoiPipelineConfig,
    smoother: &mut RoiJitterSmoother,
) -> RoiPipelineDecision {
    let quality_report = evaluate_roi_quality(&frame.quality, &config.thresholds);
    let raw_crop_bounds = mouth_crop_bounds_for_report(&frame, &quality_report, config);
    let accepted = quality_report.code == RoiDecisionCode::RoiOk;
    let smoothed_crop_bounds = if accepted {
        raw_crop_bounds.map(|crop| smoother.smooth(crop))
    } else {
        None
    };
    let quality_flags = roi_quality_flags(&quality_report, &frame.quality, config);
    let roi_request = if accepted {
        Some(build_roi_request_metadata(
            &frame,
            config,
            quality_flags.clone(),
        ))
    } else {
        None
    };
    let sidecar_decode_requests = if accepted { 1 } else { 0 };

    RoiPipelineDecision {
        code: quality_report.code,
        user_prompt_code: quality_report.code.as_str(),
        quality_flags,
        raw_crop_bounds,
        smoothed_crop_bounds,
        roi_request,
        should_emit_sidecar_decode: accepted,
        sidecar_decode_requests,
        frame_summary: RoiFrameSummary {
            observed_frames: 1,
            accepted_frames: if accepted { 1 } else { 0 },
            rejected_frames: if accepted { 0 } else { 1 },
            sidecar_decode_requests,
        },
    }
}

fn build_roi_request_metadata(
    frame: &RoiPipelineFrame,
    config: &RoiPipelineConfig,
    quality_flags: RoiQualityFlags,
) -> RoiRequestMetadata {
    RoiRequestMetadata {
        schema_version: "1.0.0".to_string(),
        request_id: frame.request_id.clone(),
        session_id: frame.session_id.clone(),
        source: RoiRequestSource {
            kind: allowed_roi_source_kind(&frame.source_kind).to_string(),
            device_id_hash: frame.device_id_hash.clone(),
            started_at_ms: frame.source_started_at_ms,
        },
        roi: NormalizedRoiClip {
            local_ref: local_roi_ref(&frame.local_ref),
            format: "grayscale_u8".to_string(),
            width: config.target_width,
            height: config.target_height,
            fps: config.fps,
            frame_count: frame.frame_count.max(1),
            duration_ms: frame.duration_ms.max(1),
            cnvsrc_center_crop_size: config.cnvsrc_center_crop_size,
            cnvsrc_compatibility_note: CNVSRC_COMPATIBILITY_NOTE.to_string(),
        },
        quality_flags,
        requested_at_ms: frame.requested_at_ms,
    }
}

fn roi_quality_flags(
    report: &RoiQualityReport,
    frame: &FrameQuality,
    config: &RoiPipelineConfig,
) -> RoiQualityFlags {
    RoiQualityFlags {
        schema_version: "1.0.0".to_string(),
        face_found: report.face_found,
        mouth_landmarks_found: report.mouth_landmarks_found,
        crop_bounds_valid: report.crop_bounds_valid,
        blur_ok: blur_passes(frame, &config.thresholds),
        brightness_ok: brightness_passes(frame, &config.thresholds),
        pose_ok: true,
        occlusion_ok: report.code != RoiDecisionCode::MouthOccluded,
        landmark_confidence: report.confidence.clamp(0.0, 1.0),
        rejection_reasons: roi_rejection_reasons(report.code),
    }
}

fn mouth_crop_bounds_for_report(
    frame: &RoiPipelineFrame,
    report: &RoiQualityReport,
    config: &RoiPipelineConfig,
) -> Option<Rect> {
    report.crop_bounds.or_else(|| {
        frame
            .quality
            .face
            .and_then(|face| mouth_crop_bounds(&face, &config.thresholds))
    })
}

fn roi_rejection_reasons(code: RoiDecisionCode) -> Vec<String> {
    match code {
        RoiDecisionCode::RoiOk => Vec::new(),
        RoiDecisionCode::NoFace => vec!["face_not_found".to_string()],
        RoiDecisionCode::MouthOccluded => vec![
            "mouth_landmarks_missing".to_string(),
            "mouth_occluded".to_string(),
        ],
        RoiDecisionCode::LowLight => vec!["brightness_out_of_range".to_string()],
        RoiDecisionCode::Blurry => vec!["blur_too_high".to_string()],
        RoiDecisionCode::CropOutOfBounds => vec!["crop_bounds_invalid".to_string()],
    }
}

fn allowed_roi_source_kind(kind: &str) -> &'static str {
    match kind {
        "camera" => "camera",
        "public_video" => "public_video",
        "fixture" => "fixture",
        _ => "fixture",
    }
}

fn local_roi_ref(local_ref: &str) -> String {
    if local_ref.starts_with("local://") {
        local_ref.to_string()
    } else {
        format!("local://roi/{local_ref}")
    }
}

fn exponential_average(previous: f32, current: f32, alpha: f32) -> f32 {
    (previous * (1.0 - alpha)) + (current * alpha)
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OverlayCandidate {
    pub text: String,
    pub source: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyState {
    Idle {
        chord: String,
    },
    CollisionRemapRequired {
        default_chord: String,
    },
    Recording {
        chord: String,
    },
    Processing {
        chord: String,
    },
    ShowingCandidates {
        chord: String,
        candidates: Vec<OverlayCandidate>,
        low_quality: bool,
        auto_insert_threshold_met: bool,
    },
}

impl Default for HotkeyState {
    fn default() -> Self {
        Self::Idle {
            chord: "Ctrl+Alt+Space".to_string(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyEvent {
    CollisionDetected,
    Remapped {
        new_chord: String,
    },
    HotkeyPressed,
    RecordingStopped,
    ProcessingComplete {
        candidates: Vec<OverlayCandidate>,
        low_quality: bool,
        auto_insert_threshold_met: bool,
    },
    NumberKeyPressed(usize), // 1-based index (1-5)
    MouseSelected(usize),    // 0-based index
    EscapePressed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HotkeyActionResult {
    None,
    InsertCandidate(OverlayCandidate),
    Cancel,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HotkeyOverlayStateMachine {
    state: HotkeyState,
}

impl HotkeyOverlayStateMachine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn state(&self) -> &HotkeyState {
        &self.state
    }

    pub fn apply(&mut self, event: HotkeyEvent) -> HotkeyActionResult {
        match (&self.state, event) {
            (HotkeyState::Idle { chord }, HotkeyEvent::CollisionDetected) => {
                self.state = HotkeyState::CollisionRemapRequired {
                    default_chord: chord.clone(),
                };
                HotkeyActionResult::None
            }
            (
                HotkeyState::CollisionRemapRequired { default_chord },
                HotkeyEvent::Remapped { new_chord },
            ) => {
                if new_chord.trim().is_empty() || new_chord == *default_chord {
                    return HotkeyActionResult::None;
                }
                self.state = HotkeyState::Idle { chord: new_chord };
                HotkeyActionResult::None
            }
            (HotkeyState::Idle { chord }, HotkeyEvent::HotkeyPressed) => {
                self.state = HotkeyState::Recording {
                    chord: chord.clone(),
                };
                HotkeyActionResult::None
            }
            (HotkeyState::Recording { chord }, HotkeyEvent::RecordingStopped) => {
                self.state = HotkeyState::Processing {
                    chord: chord.clone(),
                };
                HotkeyActionResult::None
            }
            (
                HotkeyState::Processing { chord },
                HotkeyEvent::ProcessingComplete {
                    candidates,
                    low_quality,
                    auto_insert_threshold_met,
                },
            ) => {
                let candidates = candidates.into_iter().take(5).collect();
                self.state = HotkeyState::ShowingCandidates {
                    chord: chord.clone(),
                    candidates,
                    low_quality,
                    auto_insert_threshold_met,
                };
                HotkeyActionResult::None
            }
            (
                HotkeyState::ShowingCandidates {
                    chord, candidates, ..
                },
                HotkeyEvent::NumberKeyPressed(num),
            ) => {
                if num > 0 && num <= candidates.len() {
                    let candidate = candidates[num - 1].clone();
                    self.state = HotkeyState::Idle {
                        chord: chord.clone(),
                    };
                    HotkeyActionResult::InsertCandidate(candidate)
                } else {
                    HotkeyActionResult::None
                }
            }
            (
                HotkeyState::ShowingCandidates {
                    chord, candidates, ..
                },
                HotkeyEvent::MouseSelected(index),
            ) => {
                if index < candidates.len() {
                    let candidate = candidates[index].clone();
                    self.state = HotkeyState::Idle {
                        chord: chord.clone(),
                    };
                    HotkeyActionResult::InsertCandidate(candidate)
                } else {
                    HotkeyActionResult::None
                }
            }
            (HotkeyState::ShowingCandidates { chord, .. }, HotkeyEvent::EscapePressed)
            | (HotkeyState::Recording { chord }, HotkeyEvent::EscapePressed)
            | (HotkeyState::Processing { chord }, HotkeyEvent::EscapePressed) => {
                self.state = HotkeyState::Idle {
                    chord: chord.clone(),
                };
                HotkeyActionResult::Cancel
            }
            _ => HotkeyActionResult::None,
        }
    }
}

pub const DEFAULT_AUTO_INSERT_MIN_SCORE: f32 = 0.86;
pub const DEFAULT_AUTO_INSERT_MIN_MARGIN: f32 = 0.10;
pub const DEFAULT_RERANK_CONFIDENCE_MIN: f32 = 0.80;

#[derive(Debug, Clone, PartialEq)]
pub struct FullLoopConfig {
    pub auto_insert_min_score: f32,
    pub auto_insert_min_margin: f32,
    pub rerank_confidence_min: f32,
    pub max_overlay_candidates: usize,
}

impl Default for FullLoopConfig {
    fn default() -> Self {
        Self {
            auto_insert_min_score: DEFAULT_AUTO_INSERT_MIN_SCORE,
            auto_insert_min_margin: DEFAULT_AUTO_INSERT_MIN_MARGIN,
            rerank_confidence_min: DEFAULT_RERANK_CONFIDENCE_MIN,
            max_overlay_candidates: MAX_LOCAL_RERANK_CANDIDATES,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RerankGate {
    pub enabled: bool,
    pub provider: String,
    pub confidence: Option<f32>,
}

impl RerankGate {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            provider: "local_disabled".to_string(),
            confidence: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SidecarFixtureResponse {
    pub model_id: String,
    pub runtime_id: String,
    pub latency_ms: u64,
    pub candidates: Vec<LocalRankCandidate>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SidecarDecodeResult {
    Candidates(SidecarFixtureResponse),
    Unavailable { error_code: String, message: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullLoopInsertionPlan {
    pub insert_id: String,
    pub context_before_insert: TextContext,
    pub target_app: TargetApp,
    pub candidate_text: String,
    pub report: InsertionExecutionReport,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FullLoopVisibleState {
    AutoInserted,
    CandidatesShown,
    SidecarUnavailable,
    RoiRejected,
    HotkeyBlocked,
    InsertFailed,
}

impl FullLoopVisibleState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AutoInserted => "AUTO_INSERTED",
            Self::CandidatesShown => "CANDIDATES_SHOWN",
            Self::SidecarUnavailable => "SIDECAR_UNAVAILABLE",
            Self::RoiRejected => "ROI_REJECTED",
            Self::HotkeyBlocked => "HOTKEY_BLOCKED",
            Self::InsertFailed => "INSERT_FAILED",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoInsertBlockReason {
    Passed,
    NoCandidates,
    CandidateIneligible,
    ScoreBelowThreshold,
    MarginBelowThreshold,
    RerankConfidenceBelowThreshold,
}

impl AutoInsertBlockReason {
    pub fn as_str(self) -> Option<&'static str> {
        match self {
            Self::Passed => None,
            Self::NoCandidates => Some("NO_CANDIDATES"),
            Self::CandidateIneligible => Some("CANDIDATE_INELIGIBLE"),
            Self::ScoreBelowThreshold => Some("SCORE_BELOW_THRESHOLD"),
            Self::MarginBelowThreshold => Some("MARGIN_BELOW_THRESHOLD"),
            Self::RerankConfidenceBelowThreshold => Some("RERANK_CONFIDENCE_BELOW_THRESHOLD"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ConservativeAutoInsertDecision {
    pub should_auto_insert: bool,
    pub reason: AutoInsertBlockReason,
    pub top_score: f32,
    pub margin: f32,
}

impl ConservativeAutoInsertDecision {
    pub fn reason_code(&self) -> Option<&'static str> {
        self.reason.as_str()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoopEventKind {
    HotkeyPressed,
    HotkeyBlocked,
    RoiAccepted,
    RoiRejected,
    SidecarDecodeRequested,
    SidecarDecoded,
    SidecarUnavailable,
    LocalRerankCompleted,
    AutoInsertConfirmed,
    OverlayShown,
    InsertFailed,
    SessionReset,
}

impl LoopEventKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::HotkeyPressed => "hotkey_pressed",
            Self::HotkeyBlocked => "hotkey_blocked",
            Self::RoiAccepted => "roi_accepted",
            Self::RoiRejected => "roi_rejected",
            Self::SidecarDecodeRequested => "sidecar_decode_requested",
            Self::SidecarDecoded => "sidecar_decoded",
            Self::SidecarUnavailable => "sidecar_unavailable",
            Self::LocalRerankCompleted => "local_rerank_completed",
            Self::AutoInsertConfirmed => "auto_insert_confirmed",
            Self::OverlayShown => "overlay_shown",
            Self::InsertFailed => "insert_failed",
            Self::SessionReset => "session_reset",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LoopLogEvent {
    pub kind: LoopEventKind,
    pub request_id: String,
    pub session_id: String,
    pub timestamp_ms: u64,
    pub local_ref: Option<String>,
    pub reason_code: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullLoopFixtureInput {
    pub now_ms: u64,
    pub hotkey_collision_detected: bool,
    pub roi_decision: RoiPipelineDecision,
    pub sidecar_decode: SidecarDecodeResult,
    pub dictionary_terms: Vec<DictionaryTerm>,
    pub rerank_gate: RerankGate,
    pub insertion: Option<FullLoopInsertionPlan>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FullLoopOutcome {
    pub visible_state: FullLoopVisibleState,
    pub auto_insert_decision: ConservativeAutoInsertDecision,
    pub ranked_candidates: Vec<LocalRankCandidate>,
    pub overlay_candidates: Vec<OverlayCandidate>,
    pub insert_record: Option<InsertRecord>,
    pub debug_log_event: DebugLogEvent,
    pub event_chain: Vec<LoopLogEvent>,
    pub sidecar_decode_requests: u32,
    pub hotkey_state: HotkeyState,
    pub session_reset: bool,
    pub clipboard_preserved: bool,
    pub insertion_attempted: bool,
    pub dictionary_learning_signal: Option<DictionaryLearningSignal>,
}

impl FullLoopOutcome {
    pub fn visible_state_code(&self) -> &str {
        self.debug_log_event
            .failure_reason
            .as_deref()
            .unwrap_or_else(|| self.visible_state.as_str())
    }
}

pub fn run_fixture_vsr_input_loop(
    input: FullLoopFixtureInput,
    config: &FullLoopConfig,
    insertion_state: &mut InsertionStateMachine,
) -> FullLoopOutcome {
    let request_id = loop_request_id(&input.roi_decision);
    let session_id = loop_session_id(&input.roi_decision);
    let local_ref = input
        .roi_decision
        .roi_request
        .as_ref()
        .map(|request| request.roi.local_ref.clone());
    let mut hotkey = HotkeyOverlayStateMachine::new();
    let mut event_chain = Vec::new();

    if input.hotkey_collision_detected {
        hotkey.apply(HotkeyEvent::CollisionDetected);
        event_chain.push(loop_event(
            LoopEventKind::HotkeyBlocked,
            &request_id,
            &session_id,
            input.now_ms,
            None,
            Some("HOTKEY_COLLISION"),
        ));
        return loop_outcome(
            FullLoopVisibleState::HotkeyBlocked,
            ConservativeAutoInsertDecision {
                should_auto_insert: false,
                reason: AutoInsertBlockReason::NoCandidates,
                top_score: 0.0,
                margin: 0.0,
            },
            Vec::new(),
            Vec::new(),
            None,
            debug_event(
                &request_id,
                input.now_ms,
                &input.roi_decision.quality_flags,
                &[],
                InsertionOutcome::NotAttempted,
                0,
                "none",
                Some("HOTKEY_COLLISION"),
            ),
            event_chain,
            0,
            hotkey,
            true,
            true,
            false,
            None,
        );
    }

    hotkey.apply(HotkeyEvent::HotkeyPressed);
    hotkey.apply(HotkeyEvent::RecordingStopped);
    event_chain.push(loop_event(
        LoopEventKind::HotkeyPressed,
        &request_id,
        &session_id,
        input.now_ms,
        None,
        None,
    ));

    if !input.roi_decision.should_emit_sidecar_decode {
        let reason = input.roi_decision.user_prompt_code.to_string();
        event_chain.push(loop_event(
            LoopEventKind::RoiRejected,
            &request_id,
            &session_id,
            input.now_ms,
            None,
            Some(&reason),
        ));
        hotkey.apply(HotkeyEvent::EscapePressed);
        event_chain.push(loop_event(
            LoopEventKind::SessionReset,
            &request_id,
            &session_id,
            input.now_ms,
            None,
            Some(&reason),
        ));
        return loop_outcome(
            FullLoopVisibleState::RoiRejected,
            ConservativeAutoInsertDecision {
                should_auto_insert: false,
                reason: AutoInsertBlockReason::NoCandidates,
                top_score: 0.0,
                margin: 0.0,
            },
            Vec::new(),
            Vec::new(),
            None,
            debug_event(
                &request_id,
                input.now_ms,
                &input.roi_decision.quality_flags,
                &[],
                InsertionOutcome::NotAttempted,
                0,
                "none",
                Some(&reason),
            ),
            event_chain,
            0,
            hotkey,
            true,
            true,
            false,
            None,
        );
    }

    event_chain.push(loop_event(
        LoopEventKind::RoiAccepted,
        &request_id,
        &session_id,
        input.now_ms,
        local_ref.as_deref(),
        None,
    ));
    let sidecar_decode_requests = input.roi_decision.sidecar_decode_requests;
    event_chain.push(loop_event(
        LoopEventKind::SidecarDecodeRequested,
        &request_id,
        &session_id,
        input.now_ms,
        local_ref.as_deref(),
        None,
    ));

    let sidecar_response = match input.sidecar_decode {
        SidecarDecodeResult::Candidates(response) => response,
        SidecarDecodeResult::Unavailable { error_code, .. } => {
            let reason = non_empty_reason(&error_code, "SIDECAR_UNAVAILABLE");
            event_chain.push(loop_event(
                LoopEventKind::SidecarUnavailable,
                &request_id,
                &session_id,
                input.now_ms,
                local_ref.as_deref(),
                Some(&reason),
            ));
            hotkey.apply(HotkeyEvent::EscapePressed);
            event_chain.push(loop_event(
                LoopEventKind::SessionReset,
                &request_id,
                &session_id,
                input.now_ms,
                None,
                Some(&reason),
            ));
            return loop_outcome(
                FullLoopVisibleState::SidecarUnavailable,
                ConservativeAutoInsertDecision {
                    should_auto_insert: false,
                    reason: AutoInsertBlockReason::NoCandidates,
                    top_score: 0.0,
                    margin: 0.0,
                },
                Vec::new(),
                Vec::new(),
                None,
                debug_event(
                    &request_id,
                    input.now_ms,
                    &input.roi_decision.quality_flags,
                    &[],
                    InsertionOutcome::NotAttempted,
                    0,
                    "cnvsrc2025",
                    Some(&reason),
                ),
                event_chain,
                sidecar_decode_requests,
                hotkey,
                true,
                true,
                false,
                None,
            );
        }
    };

    event_chain.push(loop_event(
        LoopEventKind::SidecarDecoded,
        &request_id,
        &session_id,
        input.now_ms.saturating_add(sidecar_response.latency_ms),
        local_ref.as_deref(),
        None,
    ));
    let ranked_candidates = rank_candidates_locally(
        &sidecar_response.candidates,
        &input.dictionary_terms,
        config.max_overlay_candidates,
    );
    event_chain.push(loop_event(
        LoopEventKind::LocalRerankCompleted,
        &request_id,
        &session_id,
        input.now_ms.saturating_add(sidecar_response.latency_ms),
        None,
        None,
    ));
    let auto_insert_decision =
        conservative_auto_insert_decision(&ranked_candidates, config, &input.rerank_gate);
    let debug_candidates =
        debug_candidate_summaries(&ranked_candidates, config.max_overlay_candidates);

    if auto_insert_decision.should_auto_insert {
        let top_candidate = ranked_candidates
            .first()
            .expect("auto insert decision requires a top candidate");
        let insertion_attempted = input.insertion.is_some();
        if let Some(plan) = input.insertion {
            if plan.candidate_text != top_candidate.text {
                let reason = "INSERTION_TEXT_MISMATCH";
                hotkey.apply(HotkeyEvent::EscapePressed);
                event_chain.push(loop_event(
                    LoopEventKind::InsertFailed,
                    &request_id,
                    &session_id,
                    input.now_ms,
                    None,
                    Some(reason),
                ));
                event_chain.push(loop_event(
                    LoopEventKind::SessionReset,
                    &request_id,
                    &session_id,
                    input.now_ms,
                    None,
                    Some(reason),
                ));
                return loop_outcome(
                    FullLoopVisibleState::InsertFailed,
                    auto_insert_decision,
                    ranked_candidates,
                    Vec::new(),
                    None,
                    debug_event(
                        &request_id,
                        input.now_ms,
                        &input.roi_decision.quality_flags,
                        &debug_candidates,
                        InsertionOutcome::Failed,
                        sidecar_response.latency_ms,
                        &sidecar_response.model_id,
                        Some(reason),
                    ),
                    event_chain,
                    sidecar_decode_requests,
                    hotkey,
                    true,
                    true,
                    insertion_attempted,
                    None,
                );
            }
            let decision = insertion_state.confirm_insertion(
                ConfirmedInsertRequest {
                    insert_id: plan.insert_id,
                    session_id: session_id.clone(),
                    candidate_text: top_candidate.text.clone(),
                    target_app: plan.target_app,
                    inserted_at_ms: input.now_ms,
                    context_before_insert: plan.context_before_insert,
                },
                plan.report,
            );
            if decision.confirmed {
                hotkey.apply(HotkeyEvent::EscapePressed);
                event_chain.push(loop_event(
                    LoopEventKind::AutoInsertConfirmed,
                    &request_id,
                    &session_id,
                    input.now_ms,
                    None,
                    None,
                ));
                return loop_outcome(
                    FullLoopVisibleState::AutoInserted,
                    auto_insert_decision,
                    ranked_candidates,
                    Vec::new(),
                    decision.record,
                    debug_event(
                        &request_id,
                        input.now_ms,
                        &input.roi_decision.quality_flags,
                        &debug_candidates,
                        InsertionOutcome::AutoInserted,
                        sidecar_response.latency_ms,
                        &sidecar_response.model_id,
                        None,
                    ),
                    event_chain,
                    sidecar_decode_requests,
                    hotkey,
                    false,
                    decision.clipboard_restored,
                    insertion_attempted,
                    Some(DictionaryLearningSignal::AutoInsertNotUndone),
                );
            }

            let reason = decision
                .reason_code()
                .unwrap_or("INSERT_FAILED")
                .to_string();
            hotkey.apply(HotkeyEvent::EscapePressed);
            event_chain.push(loop_event(
                LoopEventKind::InsertFailed,
                &request_id,
                &session_id,
                input.now_ms,
                None,
                Some(&reason),
            ));
            event_chain.push(loop_event(
                LoopEventKind::SessionReset,
                &request_id,
                &session_id,
                input.now_ms,
                None,
                Some(&reason),
            ));
            return loop_outcome(
                FullLoopVisibleState::InsertFailed,
                auto_insert_decision,
                ranked_candidates,
                Vec::new(),
                None,
                debug_event(
                    &request_id,
                    input.now_ms,
                    &input.roi_decision.quality_flags,
                    &debug_candidates,
                    InsertionOutcome::Failed,
                    sidecar_response.latency_ms,
                    &sidecar_response.model_id,
                    Some(&reason),
                ),
                event_chain,
                sidecar_decode_requests,
                hotkey,
                true,
                decision.clipboard_restored,
                insertion_attempted,
                None,
            );
        }
    }

    let overlay_candidates = overlay_candidates(&ranked_candidates, config.max_overlay_candidates);
    hotkey.apply(HotkeyEvent::ProcessingComplete {
        candidates: overlay_candidates.clone(),
        low_quality: !input
            .roi_decision
            .quality_flags
            .rejection_reasons
            .is_empty(),
        auto_insert_threshold_met: false,
    });
    let reason = auto_insert_decision.reason_code().map(str::to_string);
    event_chain.push(loop_event(
        LoopEventKind::OverlayShown,
        &request_id,
        &session_id,
        input.now_ms.saturating_add(sidecar_response.latency_ms),
        None,
        reason.as_deref(),
    ));
    loop_outcome(
        FullLoopVisibleState::CandidatesShown,
        auto_insert_decision,
        ranked_candidates,
        overlay_candidates,
        None,
        debug_event(
            &request_id,
            input.now_ms,
            &input.roi_decision.quality_flags,
            &debug_candidates,
            InsertionOutcome::CandidateShown,
            sidecar_response.latency_ms,
            &sidecar_response.model_id,
            reason.as_deref(),
        ),
        event_chain,
        sidecar_decode_requests,
        hotkey,
        false,
        true,
        false,
        None,
    )
}

pub fn loop_event_chain_is_local_only(events: &[LoopLogEvent]) -> bool {
    events.iter().all(|event| match event.local_ref.as_deref() {
        Some(value) => is_local_reference(value),
        None => true,
    })
}

fn conservative_auto_insert_decision(
    candidates: &[LocalRankCandidate],
    config: &FullLoopConfig,
    rerank_gate: &RerankGate,
) -> ConservativeAutoInsertDecision {
    let Some(top) = candidates.first() else {
        return ConservativeAutoInsertDecision {
            should_auto_insert: false,
            reason: AutoInsertBlockReason::NoCandidates,
            top_score: 0.0,
            margin: 0.0,
        };
    };
    let second_score = candidates
        .get(1)
        .map(|candidate| candidate.score)
        .unwrap_or(0.0);
    let margin = (top.score - second_score).max(0.0);
    let reason = if !top.is_auto_insert_eligible {
        AutoInsertBlockReason::CandidateIneligible
    } else if top.score < config.auto_insert_min_score {
        AutoInsertBlockReason::ScoreBelowThreshold
    } else if margin < config.auto_insert_min_margin {
        AutoInsertBlockReason::MarginBelowThreshold
    } else if rerank_gate.enabled
        && rerank_gate.confidence.unwrap_or(0.0) < config.rerank_confidence_min
    {
        AutoInsertBlockReason::RerankConfidenceBelowThreshold
    } else {
        AutoInsertBlockReason::Passed
    };

    ConservativeAutoInsertDecision {
        should_auto_insert: reason == AutoInsertBlockReason::Passed,
        reason,
        top_score: top.score,
        margin,
    }
}

fn loop_request_id(decision: &RoiPipelineDecision) -> String {
    decision
        .roi_request
        .as_ref()
        .map(|request| request.request_id.clone())
        .unwrap_or_else(|| "roi-rejected".to_string())
}

fn loop_session_id(decision: &RoiPipelineDecision) -> String {
    decision
        .roi_request
        .as_ref()
        .map(|request| request.session_id.clone())
        .unwrap_or_else(|| "session-reset".to_string())
}

fn loop_event(
    kind: LoopEventKind,
    request_id: &str,
    session_id: &str,
    timestamp_ms: u64,
    local_ref: Option<&str>,
    reason_code: Option<&str>,
) -> LoopLogEvent {
    LoopLogEvent {
        kind,
        request_id: request_id.to_string(),
        session_id: session_id.to_string(),
        timestamp_ms,
        local_ref: local_ref.map(str::to_string),
        reason_code: reason_code.map(str::to_string),
    }
}

#[allow(clippy::too_many_arguments)]
fn loop_outcome(
    visible_state: FullLoopVisibleState,
    auto_insert_decision: ConservativeAutoInsertDecision,
    ranked_candidates: Vec<LocalRankCandidate>,
    overlay_candidates: Vec<OverlayCandidate>,
    insert_record: Option<InsertRecord>,
    debug_log_event: DebugLogEvent,
    event_chain: Vec<LoopLogEvent>,
    sidecar_decode_requests: u32,
    hotkey: HotkeyOverlayStateMachine,
    session_reset: bool,
    clipboard_preserved: bool,
    insertion_attempted: bool,
    dictionary_learning_signal: Option<DictionaryLearningSignal>,
) -> FullLoopOutcome {
    FullLoopOutcome {
        visible_state,
        auto_insert_decision,
        ranked_candidates,
        overlay_candidates,
        insert_record,
        debug_log_event,
        event_chain,
        sidecar_decode_requests,
        hotkey_state: hotkey.state().clone(),
        session_reset,
        clipboard_preserved,
        insertion_attempted,
        dictionary_learning_signal,
    }
}

fn debug_event(
    request_id: &str,
    timestamp_ms: u64,
    quality_flags: &RoiQualityFlags,
    candidates: &[DebugCandidateSummary],
    insertion_outcome: InsertionOutcome,
    latency_ms: u64,
    model_id: &str,
    failure_reason: Option<&str>,
) -> DebugLogEvent {
    DebugLogEvent {
        request_id: request_id.to_string(),
        timestamp_ms,
        quality_flags: loop_quality_flag_codes(quality_flags),
        candidates: candidates.to_vec(),
        insertion_outcome,
        undo_outcome: UndoOutcome::NotRequested,
        latency_ms,
        model_id: model_id.to_string(),
        failure_reason: failure_reason.map(str::to_string),
    }
}

fn loop_quality_flag_codes(quality_flags: &RoiQualityFlags) -> Vec<String> {
    if quality_flags.rejection_reasons.is_empty() {
        vec!["ROI_OK".to_string()]
    } else {
        quality_flags.rejection_reasons.clone()
    }
}

fn debug_candidate_summaries(
    candidates: &[LocalRankCandidate],
    max_candidates: usize,
) -> Vec<DebugCandidateSummary> {
    candidates
        .iter()
        .take(max_candidates.clamp(1, MAX_LOCAL_RERANK_CANDIDATES))
        .map(|candidate| DebugCandidateSummary {
            rank: candidate.rank,
            text: candidate.text.clone(),
            score: candidate.score,
            source: candidate.source.clone(),
        })
        .collect()
}

fn overlay_candidates(
    candidates: &[LocalRankCandidate],
    max_candidates: usize,
) -> Vec<OverlayCandidate> {
    candidates
        .iter()
        .take(max_candidates.clamp(1, MAX_LOCAL_RERANK_CANDIDATES))
        .map(|candidate| OverlayCandidate {
            text: candidate.text.clone(),
            source: candidate.source.clone(),
        })
        .collect()
}

fn non_empty_reason(value: &str, fallback: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        fallback.to_string()
    } else {
        trimmed.to_string()
    }
}

fn is_local_reference(value: &str) -> bool {
    value.starts_with("local://") || !value.contains("://")
}
