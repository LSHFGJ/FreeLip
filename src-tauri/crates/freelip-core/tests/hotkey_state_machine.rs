use freelip_core::{
    HotkeyActionResult, HotkeyEvent, HotkeyOverlayStateMachine, HotkeyState, OverlayCandidate,
};

#[test]
fn hotkey_state_machine_default_idle() {
    let sm = HotkeyOverlayStateMachine::new();
    assert_eq!(
        *sm.state(),
        HotkeyState::Idle {
            chord: "Ctrl+Alt+Space".into()
        }
    );
}

#[test]
fn hotkey_state_machine_collision() {
    let mut sm = HotkeyOverlayStateMachine::new();
    sm.apply(HotkeyEvent::CollisionDetected);
    assert_eq!(
        *sm.state(),
        HotkeyState::CollisionRemapRequired {
            default_chord: "Ctrl+Alt+Space".into()
        }
    );

    // Test that HotkeyPressed is ignored while in CollisionRemapRequired
    let res = sm.apply(HotkeyEvent::HotkeyPressed);
    assert_eq!(res, HotkeyActionResult::None);
    assert_eq!(
        *sm.state(),
        HotkeyState::CollisionRemapRequired {
            default_chord: "Ctrl+Alt+Space".into()
        }
    );

    // Test that empty new_chord or same chord is rejected
    sm.apply(HotkeyEvent::Remapped {
        new_chord: "   ".into(),
    });
    assert_eq!(
        *sm.state(),
        HotkeyState::CollisionRemapRequired {
            default_chord: "Ctrl+Alt+Space".into()
        }
    );

    sm.apply(HotkeyEvent::Remapped {
        new_chord: "Ctrl+Alt+Space".into(),
    });
    assert_eq!(
        *sm.state(),
        HotkeyState::CollisionRemapRequired {
            default_chord: "Ctrl+Alt+Space".into()
        }
    );

    sm.apply(HotkeyEvent::Remapped {
        new_chord: "Ctrl+Alt+L".into(),
    });
    assert_eq!(
        *sm.state(),
        HotkeyState::Idle {
            chord: "Ctrl+Alt+L".into()
        }
    );

    // Now HotkeyPressed starts recording
    sm.apply(HotkeyEvent::HotkeyPressed);
    assert_eq!(
        *sm.state(),
        HotkeyState::Recording {
            chord: "Ctrl+Alt+L".into()
        }
    );
}

#[test]
fn hotkey_state_machine_recording_flow() {
    let mut sm = HotkeyOverlayStateMachine::new();
    sm.apply(HotkeyEvent::HotkeyPressed);
    assert_eq!(
        *sm.state(),
        HotkeyState::Recording {
            chord: "Ctrl+Alt+Space".into()
        }
    );
    sm.apply(HotkeyEvent::RecordingStopped);
    assert_eq!(
        *sm.state(),
        HotkeyState::Processing {
            chord: "Ctrl+Alt+Space".into()
        }
    );

    let candidates = vec![
        OverlayCandidate {
            text: "Candidate 1".into(),
            source: "vsr".into(),
        },
        OverlayCandidate {
            text: "Candidate 2".into(),
            source: "llm_rerank".into(),
        },
    ];
    sm.apply(HotkeyEvent::ProcessingComplete {
        candidates: candidates.clone(),
        low_quality: true,
        auto_insert_threshold_met: false,
    });

    assert_eq!(
        *sm.state(),
        HotkeyState::ShowingCandidates {
            chord: "Ctrl+Alt+Space".into(),
            candidates: candidates.clone(),
            low_quality: true,
            auto_insert_threshold_met: false
        }
    );

    // Select first candidate
    let result = sm.apply(HotkeyEvent::NumberKeyPressed(1));
    assert_eq!(
        result,
        HotkeyActionResult::InsertCandidate(candidates[0].clone())
    );
    assert_eq!(
        *sm.state(),
        HotkeyState::Idle {
            chord: "Ctrl+Alt+Space".into()
        }
    );
}

#[test]
fn hotkey_state_machine_escape_cancel() {
    let mut sm = HotkeyOverlayStateMachine::new();
    sm.apply(HotkeyEvent::HotkeyPressed);
    sm.apply(HotkeyEvent::RecordingStopped);
    let candidates = vec![OverlayCandidate {
        text: "Candidate 1".into(),
        source: "vsr".into(),
    }];
    sm.apply(HotkeyEvent::ProcessingComplete {
        candidates,
        low_quality: false,
        auto_insert_threshold_met: false,
    });
    let result = sm.apply(HotkeyEvent::EscapePressed);
    assert_eq!(result, HotkeyActionResult::Cancel);
    assert_eq!(
        *sm.state(),
        HotkeyState::Idle {
            chord: "Ctrl+Alt+Space".into()
        }
    );
}
