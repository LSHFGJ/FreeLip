use jsonschema::Draft;
use serde_json::{json, Value};
use std::fs;
use std::path::{Path, PathBuf};

const CONTRACT_FIXTURES: &[(&str, &str)] = &[
    ("candidate", "candidate.valid.json"),
    ("quality_flags", "quality_flags.valid.json"),
    ("roi_request", "roi_request.valid.json"),
    ("candidate_response", "candidate_response.valid.json"),
    ("insert_record", "insert_record.valid.json"),
    ("dictionary_entry", "dictionary_entry.valid.json"),
    ("log_event", "log_event.valid.json"),
    ("llm_rerank_request", "llm_rerank_request.valid.json"),
    ("llm_rerank_response", "llm_rerank_response.valid.json"),
];

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("repo root must resolve")
}

fn load_json(path: &Path) -> Value {
    let raw = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!("failed to read {}: {error}", path.display());
    });
    serde_json::from_str(&raw).unwrap_or_else(|error| {
        panic!("failed to parse {}: {error}", path.display());
    })
}

fn load_schema(contract_name: &str) -> Value {
    let path = repo_root()
        .join("schemas")
        .join(format!("{contract_name}.schema.json"));
    assert!(path.exists(), "missing schema: {}", path.display());
    load_json(&path)
}

fn validate(contract_name: &str, payload: &Value) -> Vec<String> {
    let schema = load_schema(contract_name);
    let validator = jsonschema::options()
        .with_draft(Draft::Draft202012)
        .build(&schema)
        .expect("schema must compile");
    validator
        .iter_errors(payload)
        .map(|error| error.to_string())
        .collect()
}

#[test]
fn contract_schema_accepts_valid_fixtures() {
    let root = repo_root();
    for (contract_name, fixture_name) in CONTRACT_FIXTURES {
        let payload = load_json(&root.join("schemas").join("fixtures").join(fixture_name));
        let errors = validate(contract_name, &payload);
        assert_eq!(errors, Vec::<String>::new(), "{contract_name} failed");
    }
}

#[test]
fn contract_schema_rejects_unknown_top_level_fields() {
    let root = repo_root();
    for (contract_name, fixture_name) in CONTRACT_FIXTURES {
        let mut payload = load_json(&root.join("schemas").join("fixtures").join(fixture_name));
        payload["unexpected_media_blob"] = json!("not allowed");

        let messages = validate(contract_name, &payload);

        assert!(
            !messages.is_empty(),
            "{contract_name} accepted unknown field"
        );
        assert!(
            messages
                .iter()
                .any(|message| message.contains("Additional properties")),
            "{contract_name} failed for unexpected reason: {messages:?}"
        );
    }
}

#[test]
fn contract_schema_rejects_media_fields_in_llm_payload() {
    let payload = load_json(
        &repo_root()
            .join("schemas")
            .join("fixtures")
            .join("llm_rerank_request.invalid_media.json"),
    );

    let messages = validate("llm_rerank_request", &payload).join("\n");

    assert!(
        messages.contains("video_bytes"),
        "missing video rejection: {messages}"
    );
    assert!(
        messages.contains("roi_frames"),
        "missing ROI rejection: {messages}"
    );
    assert!(
        messages.contains("image"),
        "missing image rejection: {messages}"
    );
    assert!(
        messages.contains("embedding"),
        "missing embedding rejection: {messages}"
    );
}

#[test]
fn contract_schema_requires_versions_on_standalone_contracts() {
    let root = repo_root();
    for (contract_name, fixture_name) in [
        ("candidate", "candidate.valid.json"),
        ("quality_flags", "quality_flags.valid.json"),
    ] {
        let mut payload = load_json(&root.join("schemas").join("fixtures").join(fixture_name));
        payload
            .as_object_mut()
            .expect("fixture must be an object")
            .remove("schema_version");

        let messages = validate(contract_name, &payload).join("\n");

        assert!(
            messages.contains("schema_version"),
            "{contract_name} did not require schema_version: {messages}"
        );
    }
}

#[test]
fn contract_schema_rejects_unknown_quality_reasons_everywhere() {
    let root = repo_root();
    for (contract_name, fixture_name) in [
        ("quality_flags", "quality_flags.valid.json"),
        ("roi_request", "roi_request.valid.json"),
        ("candidate_response", "candidate_response.valid.json"),
    ] {
        let mut payload = load_json(&root.join("schemas").join("fixtures").join(fixture_name));
        let quality_flags = if contract_name == "quality_flags" {
            &mut payload
        } else {
            payload
                .get_mut("quality_flags")
                .expect("fixture must contain quality_flags")
        };
        quality_flags["rejection_reasons"] = json!(["unexpected_quality_reason"]);

        let messages = validate(contract_name, &payload).join("\n");

        assert!(
            messages.contains("unexpected_quality_reason"),
            "{contract_name} accepted an unknown quality reason: {messages}"
        );
    }
}

#[test]
fn contract_schema_accepts_cnvsrc2025_candidate_source_everywhere() {
    let root = repo_root();
    for (contract_name, fixture_name, candidate_path) in [
        ("candidate", "candidate.valid.json", vec![]),
        (
            "candidate_response",
            "candidate_response.valid.json",
            vec!["candidates", "0"],
        ),
        (
            "llm_rerank_request",
            "llm_rerank_request.valid.json",
            vec!["candidates", "0"],
        ),
        (
            "llm_rerank_response",
            "llm_rerank_response.valid.json",
            vec!["reranked_candidates", "0"],
        ),
    ] {
        let mut payload = load_json(&root.join("schemas").join("fixtures").join(fixture_name));
        let mut candidate = &mut payload;
        for segment in candidate_path {
            candidate = if let Ok(index) = segment.parse::<usize>() {
                candidate
                    .as_array_mut()
                    .expect("candidate path segment must index an array")
                    .get_mut(index)
                    .expect("candidate index must exist")
            } else {
                candidate
                    .get_mut(segment)
                    .expect("candidate path segment must exist")
            };
        }
        candidate["source"] = json!("cnvsrc2025");

        let errors = validate(contract_name, &payload);

        assert_eq!(errors, Vec::<String>::new(), "{contract_name} failed");
    }
}
