import os
from collections.abc import Mapping
from freelip_eval.validate_suite import validate_suite, validate_sample

def test_suite_validation_full():
    suite_path = os.path.join(os.path.dirname(__file__), "../../fixtures/eval/ai_prompt_short_cn")
    result = validate_suite(suite_path)
    assert result["valid"] is True
    assert result["sample_count"] >= 100
    assert not result["errors"]

def test_validate_sample_invalid_types():
    sample: Mapping[str, object] = {
        "target_text": 123,
        "acceptable_equivalents": "not a list",
        "punctuation_policy": "unknown",
        "source": "synthetic",
        "license_note": "text-only internal eval",
        "domain_tag": "assistant",
        "split": "test"
    }
    errors = validate_sample(sample, 0)
    assert any("must be str" in e for e in errors)
    assert any("must be list" in e for e in errors)
    assert any("must be 'ignore' or 'strict'" in e for e in errors)

def test_validate_sample_missing_fields():
    sample: Mapping[str, object] = {"target_text": "hello"}
    errors = validate_sample(sample, 0)
    assert len(errors) > 1
    assert any("Missing required field 'split'" in e for e in errors)
