#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Generate PyO3 doc comments from underlying Rust documentation.

Scans crates with pyo3-stub-gen annotations, finds py_* wrapper
functions and methods in src/python/ directories, locates the doc
comment on the underlying Rust item, and writes it as the wrapper's
doc comment.

Copies section headers (# Errors, # Safety) as-is for clippy
compatibility. Drops # Panics sections with a warning since panics
must not cross the FFI boundary. Strips Rust intra-doc link brackets
and converts :: to . for Python conventions.

Usage:
    python generate_docstrings.py [--dry-run] [--crate NAME]

"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent
CRATES_DIR = ROOT / "crates"

ANNOTATED_CRATES = [
    "core",
    "model",
    "common",
    "analysis",
    "execution",
    "persistence",
    "serialization",
    "infrastructure",
    "cryptography",
    "network",
    "indicators",
    "data",
    "trading",
    "testkit",
    "backtest",
    "live",
]

ANNOTATED_ADAPTER_CRATES = [
    "architect_ax",
    "betfair",
    "binance",
    "bitmex",
    "blockchain",
    "bybit",
    "databento",
    "deribit",
    "dydx",
    "hyperliquid",
    "kraken",
    "okx",
    "polymarket",
    "sandbox",
    "tardis",
]

DROPPED_SECTIONS = {"Panics"}

BANNER_RE = re.compile(r"^-{3,}.*-{3,}$")

# Rust intra-doc link: [`Type::method`] or [`Type`](path)
INTRA_DOC_LINK_RE = re.compile(r"\[`([^`]+)`\](?:\([^)]*\))?")


def get_crate_src_dirs(crate_filter: str | None = None) -> list[tuple[str, Path]]:
    """
    Return (crate_name, src_dir) pairs for annotated crates.
    """
    dirs = []

    for name in ANNOTATED_CRATES:
        if crate_filter and name != crate_filter:
            continue
        p = CRATES_DIR / name / "src"
        if p.is_dir():
            dirs.append((name, p))

    for name in ANNOTATED_ADAPTER_CRATES:
        if crate_filter and name != crate_filter:
            continue
        p = CRATES_DIR / "adapters" / name / "src"
        if p.is_dir():
            dirs.append((name, p))

    return dirs


def collect_source_docs(src_dir: Path) -> dict[tuple[str | None, str], list[str]]:  # noqa: C901
    """
    Collect doc comments for items in a crate, excluding python/ files.

    Returns {(type_name_or_none, item_name): [doc_line, ...]} where lines
    exclude the ``///`` prefix. Free functions and type definitions use
    ``None`` as the type_name. Methods inside ``impl TypeName`` blocks use
    the enclosing type name.

    """
    docs: dict[tuple[str | None, str], list[str]] = {}

    for rs_file in sorted(src_dir.rglob("*.rs")):
        rel = rs_file.relative_to(src_dir)
        if rel.parts[0] == "python":
            continue

        lines = rs_file.read_text().splitlines()
        doc_block: list[str] = []
        in_multiline_attr = False
        current_impl: str | None = None
        brace_depth = 0
        impl_depth = 0

        for line in lines:
            stripped = line.strip()

            if in_multiline_attr:
                if stripped.endswith(("]", ")]")):
                    in_multiline_attr = False
                continue

            if stripped.startswith("///"):
                content = stripped[3:].removeprefix(" ")
                doc_block.append(content)
                continue

            if stripped.startswith("#["):
                if not (stripped.endswith(("]", ")]"))):
                    in_multiline_attr = True
                continue

            if stripped == "" or stripped.startswith("//"):
                doc_block = []
                continue

            impl_m = re.match(r"\s*impl(?:<[^>]*>)?\s+(\w+)", stripped)
            if impl_m:
                current_impl = impl_m.group(1)
                impl_entered = False

            brace_depth += stripped.count("{") - stripped.count("}")

            if current_impl is not None:
                if not impl_entered and "{" in stripped:
                    impl_depth = brace_depth
                    impl_entered = True
                elif impl_entered and brace_depth < impl_depth:
                    current_impl = None

            if doc_block:
                is_banner = all(BANNER_RE.match(l) or l == "" for l in doc_block)
                fn_m = re.match(
                    r"\s*pub(?:\([^)]*\))?\s+(?:const\s+|async\s+)?fn\s+(\w+)",
                    line,
                )

                if fn_m:
                    name = fn_m.group(1)

                    if not is_banner:
                        docs[(current_impl, name)] = list(doc_block)
                else:
                    type_m = re.match(
                        r"\s*pub(?:\([^)]*\))?\s+(?:struct|enum)\s+(\w+)",
                        line,
                    )

                    if type_m:
                        name = type_m.group(1)

                        if not is_banner:
                            docs[(None, name)] = list(doc_block)

            doc_block = []

    return docs


def transform_doc(
    doc_lines: list[str],
    source_file: str = "",
    fn_name: str = "",
    strip_errors: bool = False,
) -> list[str]:
    """
    Copy doc lines, dropping ``# Panics`` sections.

    Section headers like ``# Errors`` and ``# Safety`` are kept as-is
    for clippy compatibility. The numpydoc transformation happens later
    in the stub post-processor.

    """
    result: list[str] = []
    i = 0
    skip_section = False

    while i < len(doc_lines):
        line = doc_lines[i]

        m = re.match(r"^#\s+(\w+(?:\s+\w+)*)$", line)
        if m:
            section = m.group(1)

            dropped = DROPPED_SECTIONS | ({"Errors"} if strip_errors else set())
            if section in dropped:
                print(
                    f"  WARNING: # {section} in {fn_name} ({source_file})",
                    file=sys.stderr,
                )
                skip_section = True
                i += 1
                if i < len(doc_lines) and doc_lines[i] == "":
                    i += 1
                continue

            skip_section = False

        if not skip_section:
            line = INTRA_DOC_LINK_RE.sub(lambda m: f"`{m.group(1).replace('::', '.')}`", line)
            result.append(line)

        i += 1

    while result and result[0] == "":
        result.pop(0)
    while result and result[-1] == "":
        result.pop()

    return result


def format_as_doc_comment(doc_lines: list[str], indent: str) -> list[str]:
    """
    Format lines as Rust ``///`` doc comments with the given indentation.
    """
    formatted = []

    for line in doc_lines:
        if line:
            formatted.append(f"{indent}/// {line}")
        else:
            formatted.append(f"{indent}///")
    return formatted


def parse_pyo3_items(lines: list[str]) -> list[dict]:  # noqa: C901
    """
    Find py_* functions and methods with their doc comment ranges.

    Returns list of dicts with keys: fn_name, fn_line, impl_type,
    is_constructor, doc_start, doc_end, insert_line.

    """
    items = []
    impl_type: str | None = None
    in_pymethods = False
    brace_depth = 0
    pymethods_depth = 0

    doc_start: int | None = None
    doc_end: int | None = None
    first_attr_line: int | None = None
    has_new = False
    in_ml_attr = False

    for i, line in enumerate(lines):
        stripped = line.strip()

        if in_ml_attr:
            if stripped.endswith(("]", ")]")):
                in_ml_attr = False
            continue

        if stripped.startswith("///"):
            if doc_start is None:
                doc_start = i
            doc_end = i
            continue

        if stripped.startswith("#["):
            if first_attr_line is None:
                first_attr_line = i
            if stripped == "#[new]":
                has_new = True
            if stripped in ("#[pymethods]", "#[pyo3::pymethods]"):
                in_pymethods = True
            if not (stripped.endswith(("]", ")]"))):
                in_ml_attr = True
            continue

        m_impl = re.match(r"\s*impl\s+(\w+)", stripped)
        if m_impl and in_pymethods:
            impl_type = m_impl.group(1)
            pymethods_depth = brace_depth

        brace_depth += stripped.count("{") - stripped.count("}")

        if impl_type is not None and brace_depth <= pymethods_depth:
            impl_type = None
            in_pymethods = False

        fn_m = re.match(r"\s*(?:pub\s+)?(?:const\s+)?fn\s+(py_\w+)", line)
        if fn_m:
            insert = first_attr_line if first_attr_line is not None else i

            if doc_start is not None:
                insert = doc_start

            items.append(
                {
                    "fn_name": fn_m.group(1),
                    "fn_line": i,
                    "impl_type": impl_type,
                    "is_constructor": has_new,
                    "in_pymethods": in_pymethods,
                    "doc_start": doc_start,
                    "doc_end": doc_end,
                    "insert_line": insert,
                },
            )

        if not stripped.startswith("///") and not stripped.startswith("#["):
            doc_start = None
            doc_end = None
            first_attr_line = None
            has_new = False

    return items


def process_crate(  # noqa: C901
    crate_name: str,
    src_dir: Path,
    dry_run: bool = False,
) -> int:
    """
    Process a single crate, updating PyO3 doc comments.

    Returns number of doc comments updated.

    """
    print(f"Processing crate: {crate_name}")

    source_docs = collect_source_docs(src_dir)
    print(f"  Collected {len(source_docs)} source doc comments")

    python_dir = src_dir / "python"
    if not python_dir.is_dir():
        print("  No python/ directory, skipping")
        return 0

    total_updates = 0

    for rs_file in sorted(python_dir.rglob("*.rs")):
        text = rs_file.read_text()
        file_lines = text.splitlines()
        items = parse_pyo3_items(file_lines)

        if not items:
            continue

        updates = 0

        for item in reversed(items):
            fn_name = item["fn_name"]
            target = fn_name.removeprefix("py_")

            impl_type = item["impl_type"]

            if item["is_constructor"] and impl_type:
                source_doc = source_docs.get((None, impl_type)) or source_docs.get(
                    (impl_type, "new"),
                )
            elif impl_type:
                source_doc = source_docs.get((impl_type, target))
            elif item["in_pymethods"]:
                # Method lost its impl_type (parser brace tracking); skip
                source_doc = None
            else:
                # Standalone pyfunction (no impl block)
                source_doc = source_docs.get((None, target))

            if not source_doc:
                continue

            # Check if function returns Result/PyResult
            fn_line_str = file_lines[item["fn_line"]]
            returns_result = "Result" in fn_line_str

            transformed = transform_doc(
                source_doc,
                source_file=str(rs_file.relative_to(ROOT)),
                fn_name=fn_name,
                strip_errors=not returns_result,
            )

            if not transformed:
                continue

            fn_line_text = file_lines[item["fn_line"]]
            indent = fn_line_text[: len(fn_line_text) - len(fn_line_text.lstrip())]
            new_doc_lines = format_as_doc_comment(transformed, indent)

            if item["doc_start"] is not None:
                old_doc_lines = file_lines[item["doc_start"] : item["doc_end"] + 1]
                if old_doc_lines == new_doc_lines:
                    continue
                file_lines[item["doc_start"] : item["doc_end"] + 1] = new_doc_lines
            else:
                file_lines[item["insert_line"] : item["insert_line"]] = new_doc_lines

            updates += 1

        if updates:
            rel_path = rs_file.relative_to(ROOT)
            action = "Would update" if dry_run else "Updated"
            print(f"  {action} {updates} doc(s) in {rel_path}")

            if not dry_run:
                rs_file.write_text("\n".join(file_lines) + "\n")

        total_updates += updates

    return total_updates


def main() -> None:
    parser = argparse.ArgumentParser(
        description="Generate PyO3 doc comments from underlying Rust documentation.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Show what would change without writing files",
    )
    parser.add_argument(
        "--crate",
        dest="crate_name",
        help="Process only this crate (e.g. 'network')",
    )
    args = parser.parse_args()

    crate_dirs = get_crate_src_dirs(args.crate_name)

    if not crate_dirs:
        print("No matching crates found", file=sys.stderr)
        sys.exit(1)

    total = 0
    for name, src_dir in crate_dirs:
        total += process_crate(name, src_dir, dry_run=args.dry_run)

    prefix = "would be " if args.dry_run else ""
    print(f"\nTotal: {total} doc comments {prefix}updated")


if __name__ == "__main__":
    main()
