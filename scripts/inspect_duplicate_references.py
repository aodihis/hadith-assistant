#!/usr/bin/env python3
"""Extract representative duplicate Hadith references without modifying source data."""

from __future__ import annotations

import argparse
import json
from collections import defaultdict
from pathlib import Path
from typing import Any


Reference = tuple[str, str, str]


def canonical_hadith_number(record: dict[str, Any]) -> str:
    hadith_number = str(record.get("hadithNumber") or "").strip()
    if hadith_number:
        return hadith_number
    return str(record.get("ourHadithNumber") or "").strip()


def reference(record: dict[str, Any]) -> Reference:
    return (
        str(record.get("collection") or "").strip(),
        str(record.get("bookNumber") or "").strip(),
        canonical_hadith_number(record),
    )


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Write complete sample records whose collection/book/Hadith reference collides."
    )
    parser.add_argument(
        "input",
        type=Path,
        nargs="?",
        default=Path("data/imports/hadiths.json"),
    )
    parser.add_argument(
        "--output",
        type=Path,
        default=Path("data/imports/duplicate-reference-samples.json"),
    )
    parser.add_argument("--books", type=int, default=5)
    parser.add_argument("--groups-per-book", type=int, default=2)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.books < 1 or args.groups_per_book < 1:
        raise SystemExit("--books and --groups-per-book must be positive")

    with args.input.open("r", encoding="utf-8") as source:
        payload = json.load(source)

    records = payload.get("HadithTable")
    if not isinstance(records, list):
        raise SystemExit("input must contain a HadithTable array")

    by_reference: dict[Reference, list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        by_reference[reference(record)].append(record)

    duplicate_groups = {
        key: group for key, group in by_reference.items() if len(group) > 1
    }
    by_book: dict[tuple[str, str], list[tuple[Reference, list[dict[str, Any]]]]] = (
        defaultdict(list)
    )
    for key, group in sorted(duplicate_groups.items()):
        by_book[key[:2]].append((key, group))

    selected_books = sorted(by_book.items())[: args.books]
    samples = []
    for (collection, book_number), groups in selected_books:
        samples.append(
            {
                "collection": collection,
                "bookNumber": book_number,
                "duplicateGroupsInBook": len(groups),
                "sampleGroups": [
                    {
                        "reference": {
                            "collection": key[0],
                            "bookNumber": key[1],
                            "hadithNumber": key[2],
                        },
                        "recordCount": len(group),
                        "records": group,
                    }
                    for key, group in groups[: args.groups_per_book]
                ],
            }
        )

    report = {
        "source": str(args.input),
        "sourceRecordCount": len(records),
        "duplicateReferenceGroupCount": len(duplicate_groups),
        "recordsInDuplicateGroups": sum(map(len, duplicate_groups.values())),
        "sampledBookCount": len(samples),
        "samples": samples,
    }

    args.output.parent.mkdir(parents=True, exist_ok=True)
    with args.output.open("w", encoding="utf-8") as destination:
        json.dump(report, destination, ensure_ascii=False, indent=2)
        destination.write("\n")

    print(
        f"wrote {len(samples)} sampled books from {len(duplicate_groups)} "
        f"duplicate groups to {args.output}"
    )


if __name__ == "__main__":
    main()
