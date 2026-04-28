use freelip_core::{
    clipboard_restore_required_after_insert, process_uia_context, ConfirmedInsertRequest,
    InsertRecordStatus, InsertionExecutionReport, InsertionMethod, InsertionStateMachine,
    OperationAttempt, TargetApp, TextContext, UiaFieldSnapshot, UiaSkipReason, UndoAction,
    UndoExecutionReport, UndoReason,
};

#[test]
fn insertion_state_machine_clipboard_paste_and_send_input_fallback() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let mut machine = InsertionStateMachine::new();
    let request = insert_request("insert-clip", "帮我总结这段文字", target.clone());

    let clipboard_decision = machine.confirm_insertion(
        request.clone(),
        InsertionExecutionReport {
            clipboard_saved: true,
            ctrl_v: OperationAttempt::succeeded(),
            send_input: OperationAttempt::not_attempted(),
            clipboard_restore: OperationAttempt::succeeded(),
        },
    );

    assert!(clipboard_decision.confirmed);
    let clipboard_record = clipboard_decision
        .record
        .as_ref()
        .expect("successful Ctrl+V should confirm insertion");
    assert_eq!(clipboard_record.method, InsertionMethod::ClipboardPaste);
    assert_eq!(
        clipboard_record.undo_expires_at_ms,
        request.inserted_at_ms + 3_000
    );
    assert!(clipboard_decision.clipboard_restore_attempted);
    assert!(clipboard_decision.clipboard_restored);
    assert_eq!(
        machine
            .last_insert()
            .map(|record| record.insert_id.as_str()),
        Some("insert-clip")
    );

    let fallback_request = insert_request("insert-fallback", "帮我写一个提示词", target);
    let fallback_decision = machine.confirm_insertion(
        fallback_request,
        InsertionExecutionReport {
            clipboard_saved: true,
            ctrl_v: OperationAttempt::failed(),
            send_input: OperationAttempt::succeeded(),
            clipboard_restore: OperationAttempt::succeeded(),
        },
    );

    assert!(fallback_decision.confirmed);
    assert_eq!(
        fallback_decision
            .record
            .as_ref()
            .map(|record| record.method),
        Some(InsertionMethod::SendInput)
    );
    println!(
        "INSERTION_STATE_MACHINE inserted={} fallback_method={:?}",
        clipboard_record.candidate_text,
        fallback_decision
            .record
            .as_ref()
            .map(|record| record.method)
    );
}

#[test]
fn insertion_state_machine_does_not_confirm_without_success_or_clipboard_restore_attempt() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let mut machine = InsertionStateMachine::new();

    let failed_insert = machine.confirm_insertion(
        insert_request("insert-failed", "不会插入", target.clone()),
        InsertionExecutionReport {
            clipboard_saved: true,
            ctrl_v: OperationAttempt::failed(),
            send_input: OperationAttempt::failed(),
            clipboard_restore: OperationAttempt::succeeded(),
        },
    );

    assert!(!failed_insert.confirmed);
    assert!(failed_insert.record.is_none());
    assert!(machine.last_insert().is_none());

    let unsafe_clipboard = machine.confirm_insertion(
        insert_request("insert-no-restore", "剪贴板必须恢复", target),
        InsertionExecutionReport {
            clipboard_saved: true,
            ctrl_v: OperationAttempt::succeeded(),
            send_input: OperationAttempt::not_attempted(),
            clipboard_restore: OperationAttempt::not_attempted(),
        },
    );

    assert!(!unsafe_clipboard.confirmed);
    assert!(unsafe_clipboard.record.is_none());
    assert!(clipboard_restore_required_after_insert(&unsafe_clipboard));
    assert!(machine.last_insert().is_none());
}

#[test]
fn insertion_state_machine_undo_allowed_only_within_three_seconds_when_focus_and_text_match() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let mut machine = machine_with_confirmed_insert(target.clone(), "帮我总结这段文字");
    let record = machine
        .last_insert()
        .expect("fixture insert should be confirmed")
        .clone();

    let undo_plan = machine.plan_undo(
        text_context(target, "开头帮我总结这段文字结尾", false, false, false),
        record.inserted_at_ms + 2_999,
    );

    assert!(undo_plan.allowed);
    assert_eq!(undo_plan.action, UndoAction::DeleteInsertedText);
    assert_eq!(undo_plan.reason, None);
    assert_eq!(undo_plan.delete_start, Some(2));
    assert_eq!(undo_plan.delete_end, Some(10));
    assert_eq!(undo_plan.inserted_text.as_deref(), Some("帮我总结这段文字"));

    let result = machine.finish_undo(
        undo_plan,
        UndoExecutionReport {
            destructive_action: OperationAttempt::succeeded(),
            clipboard_restore: OperationAttempt::succeeded(),
        },
    );

    assert!(result.undone);
    assert!(result.clipboard_restored);
    assert_eq!(
        result.record.as_ref().map(|record| record.status),
        Some(InsertRecordStatus::Undone)
    );
    assert!(machine.last_insert().is_none());
}

#[test]
fn insertion_state_machine_undo_requires_clipboard_restore_success() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let mut machine = machine_with_confirmed_insert(target.clone(), "帮我总结这段文字");
    let record = machine
        .last_insert()
        .expect("fixture insert should be confirmed")
        .clone();
    let undo_plan = machine.plan_undo(
        text_context(target, "开头帮我总结这段文字结尾", false, false, false),
        record.inserted_at_ms + 1_000,
    );

    let result = machine.finish_undo(
        undo_plan,
        UndoExecutionReport {
            destructive_action: OperationAttempt::succeeded(),
            clipboard_restore: OperationAttempt::failed(),
        },
    );

    assert!(!result.undone);
    assert_eq!(result.reason, Some(UndoReason::UndoExecutionFailed));
    assert!(result.record.is_none());
    assert_eq!(
        machine.last_insert().map(|last| last.insert_id.as_str()),
        Some("insert-confirmed")
    );
}

#[test]
fn insertion_state_machine_undo_blocks_expired_focus_changed_user_typed_and_secure_contexts() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let other = target_app("code.exe", "sha256:code");
    let machine = machine_with_confirmed_insert(target.clone(), "帮我总结这段文字");
    let record = machine
        .last_insert()
        .expect("fixture insert should be confirmed")
        .clone();

    let expired = machine.plan_undo(
        text_context(
            target.clone(),
            "开头帮我总结这段文字结尾",
            false,
            false,
            false,
        ),
        record.undo_expires_at_ms + 1,
    );
    assert!(!expired.allowed);
    assert_eq!(expired.reason, Some(UndoReason::UndoExpired));

    let focus_changed = machine.plan_undo(
        text_context(other, "开头帮我总结这段文字结尾", false, false, false),
        record.inserted_at_ms + 1_000,
    );
    assert!(!focus_changed.allowed);
    assert_eq!(focus_changed.reason, Some(UndoReason::FocusChanged));

    let user_typed = machine.plan_undo(
        text_context(
            target.clone(),
            "开头帮我总结这段文字用户继续输入结尾",
            false,
            false,
            false,
        ),
        record.inserted_at_ms + 1_000,
    );
    assert!(!user_typed.allowed);
    assert_eq!(user_typed.reason, Some(UndoReason::UserTypedAfterInsert));

    let secure = machine.plan_undo(
        text_context(target, "开头帮我总结这段文字结尾", true, false, false),
        record.inserted_at_ms + 1_000,
    );
    assert!(!secure.allowed);
    assert_eq!(secure.reason, Some(UndoReason::SecureFieldSkipped));
    assert_eq!(secure.reason_code(), Some("SECURE_FIELD_SKIPPED"));
    println!(
        "UNDO_GUARDS expired={:?} focus={:?} typed={:?} secure={:?}",
        expired.reason, focus_changed.reason, user_typed.reason, secure.reason
    );
}

#[test]
fn insertion_state_machine_undo_uses_ctrl_z_only_when_declared_safe_for_last_insert() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let machine = machine_with_confirmed_insert(target.clone(), "帮我总结这段文字");
    let record = machine
        .last_insert()
        .expect("fixture insert should be confirmed")
        .clone();

    let ctrl_z_plan = machine.plan_undo(
        text_context(
            target,
            "开头帮我总结这段文字用户继续输入结尾",
            false,
            true,
            true,
        ),
        record.inserted_at_ms + 1_000,
    );

    assert!(ctrl_z_plan.allowed);
    assert_eq!(ctrl_z_plan.action, UndoAction::SendCtrlZ);
    assert_eq!(ctrl_z_plan.delete_start, None);
    assert_eq!(ctrl_z_plan.delete_end, None);
    assert_eq!(
        ctrl_z_plan.inserted_text.as_deref(),
        Some("帮我总结这段文字")
    );
}

#[test]
fn insertion_state_machine_uia_secure_password_fields_return_empty_context_with_reason_code() {
    let target = target_app("notepad.exe", "sha256:notepad");
    let secure = process_uia_context(UiaFieldSnapshot {
        target_app: target.clone(),
        text: "secret".to_string(),
        is_password: true,
        is_secure_field: false,
        is_elevated: false,
        supports_ctrl_z: true,
        ctrl_z_safe_for_last_insert: false,
    });

    assert!(secure.context.is_none());
    assert_eq!(secure.skip_reason, Some(UiaSkipReason::SecureFieldSkipped));
    assert_eq!(secure.reason_code(), Some("SECURE_FIELD_SKIPPED"));

    let normal = process_uia_context(UiaFieldSnapshot {
        target_app: target,
        text: "可编辑文本".to_string(),
        is_password: false,
        is_secure_field: false,
        is_elevated: false,
        supports_ctrl_z: true,
        ctrl_z_safe_for_last_insert: false,
    });

    let context = normal
        .context
        .expect("normal UIA field should expose context");
    assert_eq!(context.current_text, "可编辑文本");
    assert!(normal.skip_reason.is_none());
}

fn machine_with_confirmed_insert(
    target_app: TargetApp,
    candidate_text: &str,
) -> InsertionStateMachine {
    let mut machine = InsertionStateMachine::new();
    let decision = machine.confirm_insertion(
        insert_request("insert-confirmed", candidate_text, target_app),
        InsertionExecutionReport {
            clipboard_saved: true,
            ctrl_v: OperationAttempt::succeeded(),
            send_input: OperationAttempt::not_attempted(),
            clipboard_restore: OperationAttempt::succeeded(),
        },
    );
    assert!(decision.confirmed);
    machine
}

fn insert_request(
    insert_id: &str,
    candidate_text: &str,
    target_app: TargetApp,
) -> ConfirmedInsertRequest {
    let context_target_app = target_app.clone();
    ConfirmedInsertRequest {
        insert_id: insert_id.to_string(),
        session_id: "session-20260428-0010".to_string(),
        candidate_text: candidate_text.to_string(),
        target_app,
        inserted_at_ms: 1_777_339_204_700,
        context_before_insert: TextContext {
            target_app: context_target_app,
            current_text: "开头结尾".to_string(),
            selection_start: 2,
            selection_end: 2,
            is_secure_field: false,
            is_elevated: false,
            supports_ctrl_z: true,
            ctrl_z_safe_for_last_insert: false,
        },
    }
}

fn text_context(
    target_app: TargetApp,
    current_text: &str,
    is_secure_field: bool,
    supports_ctrl_z: bool,
    ctrl_z_safe_for_last_insert: bool,
) -> TextContext {
    TextContext {
        target_app,
        current_text: current_text.to_string(),
        selection_start: current_text.chars().count(),
        selection_end: current_text.chars().count(),
        is_secure_field,
        is_elevated: false,
        supports_ctrl_z,
        ctrl_z_safe_for_last_insert,
    }
}

fn target_app(process_name: &str, window_title_hash: &str) -> TargetApp {
    TargetApp {
        process_name: process_name.to_string(),
        window_title_hash: window_title_hash.to_string(),
    }
}
