use freelip_core::{
    process_roi_frame, DictionaryLearningSignal, DictionaryTerm, FaceLandmarkDetection,
    FacialLandmarks, FrameQuality, FullLoopConfig, FullLoopFixtureInput, FullLoopInsertionPlan,
    FullLoopVisibleState, InsertionExecutionReport, InsertionMethod, InsertionStateMachine,
    LocalRankCandidate, LoopEventKind, OperationAttempt, Point, Rect, RerankGate,
    RoiJitterSmoother, RoiPipelineConfig, RoiPipelineDecision, RoiPipelineFrame,
    SidecarDecodeResult, SidecarFixtureResponse, TargetApp, TextContext,
};

#[test]
fn full_loop_auto_inserts_only_when_score_and_margin_clear() {
    let mut insertion = InsertionStateMachine::new();
    let outcome = freelip_core::run_fixture_vsr_input_loop(
        FullLoopFixtureInput {
            now_ms: 1_800_000_011_000,
            hotkey_collision_detected: false,
            roi_decision: accepted_roi_decision("task-11-auto", "session-task-11-auto"),
            sidecar_decode: SidecarDecodeResult::Candidates(SidecarFixtureResponse {
                model_id: "cnvsrc2025".to_string(),
                runtime_id: "cnvsrc2025-fixture-cpu".to_string(),
                latency_ms: 42,
                candidates: vec![
                    candidate(1, "帮我总结这段文字", 0.91, true),
                    candidate(2, "帮我整理这段文字", 0.70, false),
                    candidate(3, "请总结这段文字", 0.61, false),
                ],
            }),
            dictionary_terms: vec![DictionaryTerm {
                surface: "总结".to_string(),
                weight: 0.10,
                tags: vec!["ai_prompt".to_string()],
            }],
            rerank_gate: RerankGate::disabled(),
            insertion: Some(insertion_plan("insert-task-11-auto", "帮我总结这段文字")),
        },
        &FullLoopConfig::default(),
        &mut insertion,
    );

    assert_eq!(outcome.visible_state, FullLoopVisibleState::AutoInserted);
    assert!(outcome.auto_insert_decision.should_auto_insert);
    assert_eq!(
        outcome.insert_record.as_ref().map(|record| record.method),
        Some(InsertionMethod::ClipboardPaste)
    );
    assert_eq!(
        outcome
            .insert_record
            .as_ref()
            .map(|record| record.undo_expires_at_ms),
        Some(1_800_000_014_000)
    );
    assert_eq!(
        outcome.dictionary_learning_signal,
        Some(DictionaryLearningSignal::AutoInsertNotUndone)
    );
    assert_eq!(outcome.sidecar_decode_requests, 1);
    assert!(outcome.overlay_candidates.is_empty());
    assert!(freelip_core::loop_event_chain_is_local_only(
        &outcome.event_chain
    ));
    assert_eq!(
        event_kinds(&outcome.event_chain),
        vec![
            LoopEventKind::HotkeyPressed,
            LoopEventKind::RoiAccepted,
            LoopEventKind::SidecarDecodeRequested,
            LoopEventKind::SidecarDecoded,
            LoopEventKind::LocalRerankCompleted,
            LoopEventKind::AutoInsertConfirmed,
        ]
    );
    assert_eq!(outcome.debug_log_event.candidate_count(), 3);
    assert!(outcome.debug_log_event.failure_reason.is_none());
}

#[test]
fn full_loop_shows_top_five_when_margin_is_not_clear() {
    let mut insertion = InsertionStateMachine::new();
    let outcome = freelip_core::run_fixture_vsr_input_loop(
        FullLoopFixtureInput {
            now_ms: 1_800_000_012_000,
            hotkey_collision_detected: false,
            roi_decision: accepted_roi_decision("task-11-overlay", "session-task-11-overlay"),
            sidecar_decode: SidecarDecodeResult::Candidates(SidecarFixtureResponse {
                model_id: "cnvsrc2025".to_string(),
                runtime_id: "cnvsrc2025-fixture-cpu".to_string(),
                latency_ms: 38,
                candidates: vec![
                    candidate(1, "帮我总结这段文字", 0.90, true),
                    candidate(2, "帮我整理这段文字", 0.86, true),
                    candidate(3, "请总结这段文字", 0.74, false),
                    candidate(4, "帮我改写这段文字", 0.66, false),
                    candidate(5, "帮我翻译这段文字", 0.52, false),
                    candidate(6, "第六候选不应展示", 0.40, false),
                ],
            }),
            dictionary_terms: Vec::new(),
            rerank_gate: RerankGate::disabled(),
            insertion: Some(insertion_plan("insert-task-11-overlay", "帮我总结这段文字")),
        },
        &FullLoopConfig::default(),
        &mut insertion,
    );

    assert_eq!(outcome.visible_state, FullLoopVisibleState::CandidatesShown);
    assert!(!outcome.auto_insert_decision.should_auto_insert);
    assert_eq!(
        outcome.auto_insert_decision.reason_code(),
        Some("MARGIN_BELOW_THRESHOLD")
    );
    assert_eq!(outcome.overlay_candidates.len(), 5);
    assert_eq!(outcome.overlay_candidates[0].text, "帮我总结这段文字");
    assert!(outcome.insert_record.is_none());
    assert!(!outcome.insertion_attempted);
    assert_eq!(outcome.debug_log_event.candidate_count(), 5);
    assert_eq!(
        outcome.debug_log_event.failure_reason.as_deref(),
        Some("MARGIN_BELOW_THRESHOLD")
    );
    assert_eq!(
        event_kinds(&outcome.event_chain).last(),
        Some(&LoopEventKind::OverlayShown)
    );
}

#[test]
fn sidecar_unavailable_resets_session_without_partial_insert() {
    let mut insertion = InsertionStateMachine::new();
    let outcome = freelip_core::run_fixture_vsr_input_loop(
        FullLoopFixtureInput {
            now_ms: 1_800_000_013_000,
            hotkey_collision_detected: false,
            roi_decision: accepted_roi_decision(
                "task-11-sidecar-down",
                "session-task-11-sidecar-down",
            ),
            sidecar_decode: SidecarDecodeResult::Unavailable {
                error_code: "SIDECAR_UNAVAILABLE".to_string(),
                message: "sidecar process exited before decode".to_string(),
            },
            dictionary_terms: Vec::new(),
            rerank_gate: RerankGate::disabled(),
            insertion: Some(insertion_plan("insert-task-11-sidecar-down", "不会插入")),
        },
        &FullLoopConfig::default(),
        &mut insertion,
    );

    assert_eq!(
        outcome.visible_state,
        FullLoopVisibleState::SidecarUnavailable
    );
    assert_eq!(outcome.visible_state_code(), "SIDECAR_UNAVAILABLE");
    assert_eq!(outcome.sidecar_decode_requests, 1);
    assert!(outcome.session_reset);
    assert!(outcome.clipboard_preserved);
    assert!(!outcome.insertion_attempted);
    assert!(outcome.insert_record.is_none());
    assert!(insertion.last_insert().is_none());
    assert_eq!(
        outcome.debug_log_event.failure_reason.as_deref(),
        Some("SIDECAR_UNAVAILABLE")
    );
    assert_eq!(
        event_kinds(&outcome.event_chain),
        vec![
            LoopEventKind::HotkeyPressed,
            LoopEventKind::RoiAccepted,
            LoopEventKind::SidecarDecodeRequested,
            LoopEventKind::SidecarUnavailable,
            LoopEventKind::SessionReset,
        ]
    );
}

#[test]
fn auto_insert_fails_closed_when_insertion_plan_text_does_not_match_top_candidate() {
    let mut insertion = InsertionStateMachine::new();
    let outcome = freelip_core::run_fixture_vsr_input_loop(
        FullLoopFixtureInput {
            now_ms: 1_800_000_013_500,
            hotkey_collision_detected: false,
            roi_decision: accepted_roi_decision("task-11-mismatch", "session-task-11-mismatch"),
            sidecar_decode: SidecarDecodeResult::Candidates(SidecarFixtureResponse {
                model_id: "cnvsrc2025".to_string(),
                runtime_id: "cnvsrc2025-fixture-cpu".to_string(),
                latency_ms: 35,
                candidates: vec![
                    candidate(1, "帮我总结这段文字", 0.92, true),
                    candidate(2, "帮我整理这段文字", 0.70, false),
                ],
            }),
            dictionary_terms: Vec::new(),
            rerank_gate: RerankGate::disabled(),
            insertion: Some(insertion_plan("insert-task-11-mismatch", "错误的插入文本")),
        },
        &FullLoopConfig::default(),
        &mut insertion,
    );

    assert_eq!(outcome.visible_state, FullLoopVisibleState::InsertFailed);
    assert!(outcome.auto_insert_decision.should_auto_insert);
    assert!(outcome.insertion_attempted);
    assert!(outcome.insert_record.is_none());
    assert!(insertion.last_insert().is_none());
    assert_eq!(
        outcome.debug_log_event.failure_reason.as_deref(),
        Some("INSERTION_TEXT_MISMATCH")
    );
    assert_eq!(
        event_kinds(&outcome.event_chain).last(),
        Some(&LoopEventKind::SessionReset)
    );
}

#[test]
fn rejected_roi_never_decodes_and_event_chain_stays_local() {
    let mut insertion = InsertionStateMachine::new();
    let outcome = freelip_core::run_fixture_vsr_input_loop(
        FullLoopFixtureInput {
            now_ms: 1_800_000_014_000,
            hotkey_collision_detected: false,
            roi_decision: rejected_roi_decision("task-11-reject", "session-task-11-reject"),
            sidecar_decode: SidecarDecodeResult::Candidates(SidecarFixtureResponse {
                model_id: "cnvsrc2025".to_string(),
                runtime_id: "cnvsrc2025-fixture-cpu".to_string(),
                latency_ms: 1,
                candidates: vec![candidate(1, "不应解码", 0.99, true)],
            }),
            dictionary_terms: Vec::new(),
            rerank_gate: RerankGate::disabled(),
            insertion: Some(insertion_plan("insert-task-11-reject", "不应插入")),
        },
        &FullLoopConfig::default(),
        &mut insertion,
    );

    assert_eq!(outcome.visible_state, FullLoopVisibleState::RoiRejected);
    assert_eq!(outcome.visible_state_code(), "NO_FACE");
    assert_eq!(outcome.sidecar_decode_requests, 0);
    assert!(outcome.insert_record.is_none());
    assert!(!outcome.insertion_attempted);
    assert!(freelip_core::loop_event_chain_is_local_only(
        &outcome.event_chain
    ));
    assert_eq!(
        event_kinds(&outcome.event_chain),
        vec![
            LoopEventKind::HotkeyPressed,
            LoopEventKind::RoiRejected,
            LoopEventKind::SessionReset
        ]
    );
}

fn accepted_roi_decision(request_id: &str, session_id: &str) -> RoiPipelineDecision {
    roi_decision(request_id, session_id, valid_frame())
}

fn rejected_roi_decision(request_id: &str, session_id: &str) -> RoiPipelineDecision {
    roi_decision(
        request_id,
        session_id,
        FrameQuality {
            face: None,
            ..valid_frame()
        },
    )
}

fn roi_decision(request_id: &str, session_id: &str, quality: FrameQuality) -> RoiPipelineDecision {
    let config = RoiPipelineConfig::default();
    let mut smoother = RoiJitterSmoother::new(config.smoothing_alpha);
    process_roi_frame(
        RoiPipelineFrame {
            request_id: request_id.to_string(),
            session_id: session_id.to_string(),
            source_kind: "fixture".to_string(),
            device_id_hash: None,
            source_started_at_ms: 1_800_000_010_000,
            requested_at_ms: 1_800_000_010_900,
            frame_count: 75,
            duration_ms: 3_000,
            local_ref: format!("local://roi/{session_id}/{request_id}.json"),
            quality,
        },
        &config,
        &mut smoother,
    )
}

fn valid_frame() -> FrameQuality {
    FrameQuality {
        frame_width: 640,
        frame_height: 480,
        brightness: 0.66,
        blur_score: 0.80,
        face: Some(FaceLandmarkDetection {
            face_bounds: Rect {
                x: 210.0,
                y: 80.0,
                width: 220.0,
                height: 260.0,
            },
            landmarks: FacialLandmarks {
                right_eye: Some(Point { x: 270.0, y: 165.0 }),
                left_eye: Some(Point { x: 370.0, y: 166.0 }),
                nose_tip: Some(Point { x: 322.0, y: 222.0 }),
                right_mouth_corner: Some(Point { x: 285.0, y: 282.0 }),
                left_mouth_corner: Some(Point { x: 360.0, y: 283.0 }),
            },
            confidence: 0.94,
        }),
    }
}

fn candidate(rank: u8, text: &str, score: f32, eligible: bool) -> LocalRankCandidate {
    LocalRankCandidate::new(rank, text, score, "cnvsrc2025", eligible)
}

fn insertion_plan(insert_id: &str, candidate_text: &str) -> FullLoopInsertionPlan {
    FullLoopInsertionPlan {
        insert_id: insert_id.to_string(),
        context_before_insert: TextContext {
            target_app: target_app(),
            current_text: "开头结尾".to_string(),
            selection_start: 2,
            selection_end: 2,
            is_secure_field: false,
            is_elevated: false,
            supports_ctrl_z: true,
            ctrl_z_safe_for_last_insert: false,
        },
        target_app: target_app(),
        candidate_text: candidate_text.to_string(),
        report: InsertionExecutionReport {
            clipboard_saved: true,
            ctrl_v: OperationAttempt::succeeded(),
            send_input: OperationAttempt::not_attempted(),
            clipboard_restore: OperationAttempt::succeeded(),
        },
    }
}

fn target_app() -> TargetApp {
    TargetApp {
        process_name: "notepad.exe".to_string(),
        window_title_hash: "sha256:notepad-task-11".to_string(),
    }
}

fn event_kinds(events: &[freelip_core::LoopLogEvent]) -> Vec<LoopEventKind> {
    events.iter().map(|event| event.kind).collect()
}
