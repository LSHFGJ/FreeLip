use freelip_core::{
    rank_candidates_locally, DictionaryLearningSignal, DictionaryTerm, LocalRankCandidate,
    PersonalDictionary,
};

#[test]
fn dictionary_learning_weights() {
    let mut dictionary = PersonalDictionary::new();

    let manual = dictionary.learn(
        "帮我总结这段文字",
        DictionaryLearningSignal::ManualSelection,
        1_800_000_000_000,
    );
    let correction = dictionary.learn(
        "帮我润色提示词",
        DictionaryLearningSignal::ManualCorrection,
        1_800_000_000_001,
    );
    let auto = dictionary.learn(
        "帮我整理这段文字",
        DictionaryLearningSignal::AutoInsertNotUndone,
        1_800_000_000_002,
    );
    let undo = dictionary.learn(
        "帮我删除这段文字",
        DictionaryLearningSignal::UndoWithinThreeSeconds,
        1_800_000_000_003,
    );

    assert!(manual.weight > auto.weight);
    assert!(correction.weight > auto.weight);
    assert!(auto.weight > undo.weight);
    assert!((0.0..=1.0).contains(&manual.weight));
    assert!((0.0..=1.0).contains(&undo.weight));

    let boosted = dictionary.learn(
        "帮我总结这段文字",
        DictionaryLearningSignal::ManualSelection,
        1_800_000_000_004,
    );
    assert_eq!(boosted.entry_id, manual.entry_id);
    assert!(boosted.weight <= 1.0);
    assert!(boosted.weight > manual.weight);

    let ranked = rank_candidates_locally(
        &[
            LocalRankCandidate::new(1, "无关候选", 0.79, "cnvsrc2025", true),
            LocalRankCandidate::new(2, "帮我总结这段文字", 0.70, "cnvsrc2025", true),
            LocalRankCandidate::new(3, "帮我整理这段文字", 0.70, "vsr", false),
            LocalRankCandidate::new(4, "第四候选", 0.60, "vsr", false),
            LocalRankCandidate::new(5, "第五候选", 0.50, "vsr", false),
            LocalRankCandidate::new(6, "第六候选", 0.40, "vsr", false),
        ],
        &dictionary.dictionary_terms(),
        5,
    );

    assert_eq!(ranked.len(), 5);
    assert_eq!(ranked[0].text, "帮我总结这段文字");
    assert_eq!(ranked[0].rank, 1);
    assert_eq!(ranked[1].rank, 2);
    assert_eq!(ranked[4].rank, 5);
    assert!(!ranked.iter().any(|candidate| candidate.text == "第六候选"));

    println!(
        "DICTIONARY_WEIGHTS manual={:.2} correction={:.2} auto={:.2} undo={:.2} boosted_rank={}",
        manual.weight, correction.weight, auto.weight, undo.weight, ranked[0].rank
    );
}

#[test]
fn dictionary_clear_export_delete() {
    let mut dictionary = PersonalDictionary::new();
    let first = dictionary.learn(
        "提示词",
        DictionaryLearningSignal::ManualCorrection,
        1_800_000_000_000,
    );
    let second = dictionary.learn_with_metadata(
        "FreeLip",
        Some("freelip"),
        &["product", "ai_prompt"],
        DictionaryLearningSignal::AutoInsertNotUndone,
        1_800_000_000_100,
    );

    let exported = dictionary.export_entries();
    assert_eq!(exported.len(), 2);
    assert_eq!(exported[0].schema_version, "1.0.0");
    assert!(exported
        .iter()
        .any(|entry| entry.entry_id == first.entry_id));
    assert!(exported
        .iter()
        .any(|entry| entry.entry_id == second.entry_id));
    assert!(exported
        .iter()
        .all(|entry| (0.0..=1.0).contains(&entry.weight)));

    let terms = dictionary.dictionary_terms();
    assert_eq!(terms.len(), 2);
    assert!(terms.iter().any(|term| term
        == &DictionaryTerm {
            surface: "FreeLip".to_string(),
            weight: second.weight,
            tags: vec!["product".to_string(), "ai_prompt".to_string()],
        }));

    let deleted = dictionary.delete_entry(&first.entry_id);
    assert_eq!(
        deleted.as_ref().map(|entry| entry.entry_id.as_str()),
        Some(first.entry_id.as_str())
    );
    assert_eq!(dictionary.export_entries().len(), 1);
    assert!(dictionary.delete_entry(&first.entry_id).is_none());

    let removed_count = dictionary.clear();
    assert_eq!(removed_count, 1);
    assert!(dictionary.export_entries().is_empty());
}
