"""Build a small, deterministic hadith fixture for the test environment.

`/data/imports/*` is gitignored pending a licensing review, so the fixture
itself is deliberately NOT committed. This script is committed instead: anyone
with the full dump can regenerate a byte-identical fixture, and no canonical
religious text enters version control.

Records are selected, never authored. Selection prefers records that carry both
an Arabic and an English grade so the grade badge has something real to render,
and the sample is sorted by a stable key so repeated runs produce the same file.

Usage:
    python scripts/make_test_fixture.py \
        --source data/imports/hadiths.json \
        --out data/imports/test-hadiths-50.json \
        --count 50 \
        --collection bukhari
"""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

REQUIRED_TEXT_FIELDS = ("arabicText", "englishText")
GRADE_FIELDS = ("arabicgrade1", "englishgrade1")


def is_usable(record: dict) -> bool:
    """A fixture record must be able to exercise the whole card UI."""
    for field in REQUIRED_TEXT_FIELDS:
        value = record.get(field)
        if not value or not value.strip():
            return False
    for field in GRADE_FIELDS:
        value = record.get(field)
        if not value or not value.strip():
            return False
    return True


def sort_key(record: dict) -> tuple:
    # Stable, content-derived ordering so the fixture does not churn between
    # runs and diffs stay meaningful.
    return (
        str(record.get("collection", "")),
        int(record.get("arabicURN") or 0),
        str(record.get("hadithNumber", "")),
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--source", default="data/imports/hadiths.json")
    parser.add_argument("--out", default="data/imports/test-hadiths-50.json")
    parser.add_argument("--count", type=int, default=50)
    parser.add_argument(
        "--collection",
        default="bukhari",
        help="collection slug to sample from; bukhari and muslim are fully graded",
    )
    args = parser.parse_args()

    source = Path(args.source)
    if not source.exists():
        print(f"source dump not found: {source}", file=sys.stderr)
        return 1

    with source.open(encoding="utf-8") as handle:
        dump = json.load(handle)

    table = dump.get("HadithTable")
    if not isinstance(table, list):
        print("source dump has no HadithTable array", file=sys.stderr)
        return 1

    candidates = [
        record
        for record in table
        if record.get("collection") == args.collection and is_usable(record)
    ]

    if len(candidates) < args.count:
        print(
            f"only {len(candidates)} usable records in `{args.collection}`, "
            f"needed {args.count}",
            file=sys.stderr,
        )
        return 1

    candidates.sort(key=sort_key)
    sample = candidates[: args.count]

    out = Path(args.out)
    out.parent.mkdir(parents=True, exist_ok=True)
    with out.open("w", encoding="utf-8") as handle:
        json.dump({"HadithTable": sample}, handle, ensure_ascii=False, indent=2)
        handle.write("\n")

    graded = sum(1 for record in sample if is_usable(record))
    print(f"wrote {len(sample)} records to {out} ({graded} with both grades)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
