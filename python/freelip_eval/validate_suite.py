import json
import os
import argparse
import sys
from collections.abc import Mapping
from typing import TypedDict, cast

class ValidationReport(TypedDict):
    suite_path: str
    sample_count: int
    errors: list[str]
    valid: bool

def validate_sample(sample: Mapping[str, object], index: int) -> list[str]:
    errors: list[str] = []
    schema: dict[str, type] = {
        "target_text": str,
        "acceptable_equivalents": list,
        "punctuation_policy": str,
        "source": str,
        "license_note": str,
        "domain_tag": str,
        "split": str
    }
    
    for field, expected_type in schema.items():
        if field not in sample:
            errors.append(f"Sample {index}: Missing required field '{field}'")
        else:
            value = sample[field]
            if not isinstance(value, expected_type):
                errors.append(f"Sample {index}: Field '{field}' must be {expected_type.__name__}")
            
    if "acceptable_equivalents" in sample:
        equivalents = sample["acceptable_equivalents"]
        if isinstance(equivalents, list):
            for i, eq in enumerate(equivalents):
                if not isinstance(eq, str):
                    errors.append(f"Sample {index}: 'acceptable_equivalents'[{i}] must be a string")
                
    if "punctuation_policy" in sample:
        policy = sample["punctuation_policy"]
        if policy not in ["ignore", "strict"]:
            errors.append(f"Sample {index}: 'punctuation_policy' must be 'ignore' or 'strict'")
        
    return errors

def validate_suite(suite_path: str) -> ValidationReport:
    report: ValidationReport = {
        "suite_path": suite_path,
        "sample_count": 0,
        "errors": [],
        "valid": False
    }
    
    samples: list[Mapping[str, object]] = []
    if os.path.isdir(suite_path):
        filenames = sorted([f for f in os.listdir(suite_path) if f.endswith(".json") and f != "metadata.json"])
        for filename in filenames:
            file_path = os.path.join(suite_path, filename)
            try:
                with open(file_path, 'r', encoding='utf-8') as f:
                    data = cast(object, json.load(f))
                    if isinstance(data, list):
                        for item in data:
                            if isinstance(item, Mapping):
                                samples.append(cast(Mapping[str, object], item))
                    elif isinstance(data, Mapping):
                        samples.append(cast(Mapping[str, object], data))
            except (json.JSONDecodeError, IOError) as e:
                report["errors"].append(f"Error reading {filename}: {str(e)}")
    
    report["sample_count"] = len(samples)
    for i, sample in enumerate(samples):
        sample_errors = validate_sample(sample, i)
        report["errors"].extend(sample_errors)
    
    if report["sample_count"] >= 100 and not report["errors"]:
        report["valid"] = True
        
    return report

if __name__ == "__main__":
    parser = argparse.ArgumentParser()
    _ = parser.add_argument("suite_path")
    _ = parser.add_argument("--report", required=True)
    args = parser.parse_args()
    
    suite_arg = str(args.suite_path)
    report_arg = str(args.report)
    
    result = validate_suite(suite_arg)
    
    report_dir = os.path.dirname(report_arg)
    if report_dir and not os.path.exists(report_dir):
        os.makedirs(report_dir)
        
    with open(report_arg, 'w', encoding='utf-8') as f:
        json.dump(result, f, indent=2, ensure_ascii=False)
    
    if result["valid"]:
        print(f"Validation successful: {result['sample_count']} samples found.")
        sys.exit(0)
    else:
        print(f"Validation failed: {len(result['errors'])} errors found. Count: {result['sample_count']}")
        for err in result["errors"][:10]:
            print(f"  - {err}")
        sys.exit(1)

