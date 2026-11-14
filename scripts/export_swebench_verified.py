#!/usr/bin/env python3
"""Export real SWE-bench Verified tasks to the DOMINION Rust runner.
Gold patches, reference solutions and hidden test patches are never exported.
Requires: pip install datasets
"""
import argparse
import hashlib
import json
from pathlib import Path

DATASET = "princeton-nlp/SWE-bench_Verified"
PUBLIC_INPUT_FIELDS = ("instance_id", "repo", "base_commit", "problem_statement")

def export_records(records, requested_ids=(), limit=5):
    requested = set(requested_ids)
    if requested and len(requested) != len(requested_ids):
        raise ValueError("duplicate requested instance IDs")
    picked = []
    seen = set()
    for row in records:
        instance = row.get("instance_id", "")
        if requested and instance not in requested:
            continue
        if instance in seen:
            raise ValueError(f"duplicate dataset instance {instance}")
        seen.add(instance)
        result = {key: row[key] for key in PUBLIC_INPUT_FIELDS}
        if not all(isinstance(value, str) and value for value in result.values()):
            raise ValueError(f"invalid SWE-bench input {instance}")
        picked.append(result)
        if not requested and len(picked) >= limit:
            break
    if requested and seen != requested:
        raise ValueError(f"requested instances missing from dataset: {sorted(requested-seen)}")
    if not picked:
        raise ValueError("no instances selected")
    return picked

def main():
    parser = argparse.ArgumentParser(description="Frozen, gold-free SWE-bench Verified export")
    parser.add_argument("--output", type=Path, required=True, help="new JSONL path")
    parser.add_argument("--limit", type=int, default=5)
    parser.add_argument("--instance-id", action="append", default=[])
    args = parser.parse_args()
    if args.limit < 1 or args.limit > 500:
        parser.error("--limit must be in 1..500")
    if args.output.exists():
        parser.error("output already exists: refusing to replace frozen task set")
    from datasets import load_dataset
    data = load_dataset(DATASET, split="test")
    records = export_records(data, args.instance_id, args.limit)
    args.output.parent.mkdir(parents=True, exist_ok=True)
    payload = "".join(json.dumps(row, ensure_ascii=False, sort_keys=True) + "\n"
                      for row in records)
    args.output.write_text(payload, encoding="utf-8")
    checksum = hashlib.sha256(payload.encode("utf-8")).hexdigest()
    meta = {"dataset": DATASET, "split": "test", "count": len(records),
            "instanceIds": [row["instance_id"] for row in records],
            "exportSha256": checksum,
            "containsGoldPatches": False,
            "containsHiddenTests": False}
    meta_path = args.output.with_suffix(args.output.suffix + ".manifest.json")
    meta_path.write_text(json.dumps(meta, indent=2) + "\n", encoding="utf-8")
    print(json.dumps({"taskFile": str(args.output), **meta}, ensure_ascii=False))

if __name__ == "__main__":
    main()
