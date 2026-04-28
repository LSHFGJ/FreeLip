use freelip_core::{
    default_debug_log_dir, plan_debug_log_retention, DebugCandidateSummary, DebugLogEvent,
    DebugLogFileRecord, DebugLogRetentionPolicy, InsertionOutcome, RoiDebugMetadata, UndoOutcome,
    DEFAULT_DEBUG_LOG_RETENTION_DAYS,
};
use std::path::PathBuf;

const DAY_MS: u64 = 24 * 60 * 60 * 1_000;

#[test]
fn debug_log_retention_expires_only_files_older_than_seven_days() {
    let now_ms = 1_800_000_000_000;
    let expired = record("expired-roi-debug.json", now_ms - (8 * DAY_MS), 200);
    let recent = record("recent-roi-debug.json", now_ms - DAY_MS, 200);
    let current_day = record("current-day-roi-debug.json", now_ms, 200);

    let plan = plan_debug_log_retention(
        &[expired.clone(), recent.clone(), current_day.clone()],
        now_ms,
        DebugLogRetentionPolicy {
            max_age_days: DEFAULT_DEBUG_LOG_RETENTION_DAYS,
            max_total_bytes: 10_000,
        },
    );

    assert_eq!(plan.expired_files, vec![expired.path]);
    assert_eq!(plan.size_cap_files, Vec::<PathBuf>::new());
    assert!(plan.retained_files.contains(&recent.path));
    assert!(plan.retained_files.contains(&current_day.path));
    assert!(!plan
        .retained_files
        .contains(&PathBuf::from("expired-roi-debug.json")));
}

#[test]
fn debug_log_retention_size_cap_removes_oldest_retained_files_first() {
    let now_ms = 1_800_000_000_000;
    let oldest = record("oldest.log", now_ms - (3 * DAY_MS), 70);
    let middle = record("middle.log", now_ms - (2 * DAY_MS), 60);
    let newest = record("newest.log", now_ms - DAY_MS, 50);

    let plan = plan_debug_log_retention(
        &[newest.clone(), oldest.clone(), middle.clone()],
        now_ms,
        DebugLogRetentionPolicy {
            max_age_days: DEFAULT_DEBUG_LOG_RETENTION_DAYS,
            max_total_bytes: 110,
        },
    );

    assert_eq!(plan.expired_files, Vec::<PathBuf>::new());
    assert_eq!(plan.size_cap_files, vec![oldest.path]);
    assert_eq!(plan.retained_bytes, 110);
    assert!(plan.retained_files.contains(&middle.path));
    assert!(plan.retained_files.contains(&newest.path));
}

#[test]
fn debug_log_retention_policy_rejects_windows_longer_than_seven_days() {
    let policy = DebugLogRetentionPolicy::new(DEFAULT_DEBUG_LOG_RETENTION_DAYS + 1, 10_000);

    assert!(policy.is_err());
}

#[test]
fn debug_log_retention_structures_hold_local_metadata_without_media_bytes() {
    let app_data_root = PathBuf::from("app-data-root");
    let debug_dir = default_debug_log_dir(&app_data_root);
    let clip_path = debug_dir.join("request-42.roi-debug.mp4");
    let metadata = RoiDebugMetadata {
        request_id: "request-42".to_string(),
        session_id: "session-20260428-0001".to_string(),
        created_timestamp_ms: 1_800_000_000_000,
        local_clip_path: Some(clip_path.clone()),
        quality_flags: vec!["ROI_OK".to_string()],
        frame_count: 42,
        duration_ms: 1_680,
    };

    assert!(clip_path.starts_with(&debug_dir));
    assert_eq!(
        metadata.local_clip_path.as_deref(),
        Some(clip_path.as_path())
    );
    assert_eq!(metadata.quality_flags, vec!["ROI_OK"]);

    let event = DebugLogEvent {
        request_id: metadata.request_id.clone(),
        timestamp_ms: metadata.created_timestamp_ms,
        quality_flags: metadata.quality_flags.clone(),
        candidates: vec![DebugCandidateSummary {
            rank: 1,
            text: "帮我总结这段文字".to_string(),
            score: 0.82,
            source: "cnvsrc2025".to_string(),
        }],
        insertion_outcome: InsertionOutcome::AutoInserted,
        undo_outcome: UndoOutcome::NotRequested,
        latency_ms: 930,
        model_id: "cnvsrc2025".to_string(),
        failure_reason: None,
    };

    assert_eq!(event.request_id, "request-42");
    assert_eq!(event.candidate_count(), 1);
    assert_eq!(event.candidates[0].text, "帮我总结这段文字");
    assert_eq!(event.failure_reason, None);
}

fn record(name: &str, modified_timestamp_ms: u64, size_bytes: u64) -> DebugLogFileRecord {
    DebugLogFileRecord {
        path: PathBuf::from(name),
        modified_timestamp_ms,
        size_bytes,
    }
}
