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

Copies Rust documentation to PyO3 wrappers in crates with pyo3-stub-gen annotations.
Explicit type mappings also copy field, enum variant, and unprefixed method documentation.
Method mappings cover renamed methods and configuration properties.

Copies section headers (# Errors, # Safety) as-is for clippy
compatibility. Drops # Panics sections since panics must not cross the
FFI boundary. Strips Rust intra-doc link brackets and converts :: to .
for Python conventions.

Usage:
    python generate_docstrings.py [--dry-run] [--crate NAME] [--verbose]

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
    "blockchain",
    "bybit",
    "databento",
    "deribit",
    "derive",
    "dydx",
    "finfeed",
    "hyperliquid",
    "kraken",
    "okx",
    "polymarket",
    "sandbox",
    "tardis",
]

DROPPED_SECTIONS = {"Panics"}

TYPE_DOC_SOURCES = {
    "FinFeedDataClientConfig": "FinFeedDataClientConfig",
    "FinFeedDataClientFactory": "FinFeedDataClientFactory",
    "FinFeedInstrumentSelection": "FinFeedInstrumentSelection",
    "ListingsQuery": "ListingsQuery",
    "ActiveQuery": "ActiveQuery",
    "ActivityQuery": "ActivityQuery",
    "HistoryQuery": "HistoryQuery",
    "FlatFileOhlcvQuery": "FlatFileOhlcvQuery",
    "OhlcvQuery": "OhlcvQuery",
    "Pagination": "Pagination",
    "PyFinFeedHttpClient": "FinFeedHttpClient",
    "PyFinFeedExchange": "Exchange",
    "PyFinFeedMarket": "Market",
    "PyFinFeedMarketsPage": "MarketsPage",
    "PyFinFeedTrade": "Trade",
    "PyFinFeedQuote": "Quote",
    "PyFinFeedActivityCurrent": "ActivityCurrent",
    "PyFinFeedActivityBatch": "ActivityBatch",
    "PyFinFeedBookLevel": "BookLevel",
    "PyFinFeedBook": "Book",
    "PyFinFeedBookEvent": "BookEvent",
    "PyFinFeedOhlcvPeriod": "OhlcvPeriod",
    "PyFinFeedOhlcv": "Ohlcv",
    "FinFeedMarketStatus": "MarketStatus",
    "FinFeedOutcomeType": "OutcomeType",
    "FinFeedTradeSide": "TradeSide",
}


METHOD_DOC_SOURCES = {
    ("FlatFileOhlcvQuery", "limit"): ("HistoryQuery", "limit"),
    ("FlatFileOhlcvQuery", "date"): ("HistoryQuery", "date"),
    ("FlatFileOhlcvQuery", "time_start"): ("HistoryQuery", "time_start"),
    ("FlatFileOhlcvQuery", "time_end"): ("HistoryQuery", "time_end"),
    ("PyFinFeedMarket", "to_instrument"): (None, "parse_instrument"),
    ("FinFeedDataClientConfig", "base_url_http"): ("FinFeedHttpConfig", "base_url"),
    ("FinFeedDataClientConfig", "http_timeout_secs"): ("FinFeedHttpConfig", "timeout_secs"),
    ("FinFeedDataClientConfig", "max_concurrent_requests"): (
        "FinFeedHttpConfig",
        "max_concurrent_requests",
    ),
    ("FinFeedDataClientConfig", "attempt_budget"): ("FinFeedHttpConfig", "attempt_budget"),
    ("FinFeedDataClientConfig", "min_request_interval_ms"): (
        "FinFeedHttpConfig",
        "min_request_interval_ms",
    ),
    ("FinFeedDataClientConfig", "has_api_key"): ("FinFeedDataClientConfig", "api_key"),
    ("FinFeedDataClientConfig", "has_proxy_url"): ("FinFeedHttpConfig", "proxy_url"),
    ("FinFeedDataClientConfig", "max_retries"): ("network::RetryConfig", "max_retries"),
    ("FinFeedDataClientConfig", "retry_delay_initial_ms"): (
        "network::RetryConfig",
        "initial_delay_ms",
    ),
    ("FinFeedDataClientConfig", "retry_delay_max_ms"): ("network::RetryConfig", "max_delay_ms"),
    ("FinFeedDataClientConfig", "retry_backoff_factor"): ("network::RetryConfig", "backoff_factor"),
    ("FinFeedDataClientConfig", "retry_jitter_ms"): ("network::RetryConfig", "jitter_ms"),
    ("FinFeedDataClientConfig", "retry_operation_timeout_ms"): (
        "FinFeedHttpConfig",
        "retry",
    ),
    ("FinFeedDataClientConfig", "retry_immediate_first"): (
        "network::RetryConfig",
        "immediate_first",
    ),
    ("FinFeedDataClientConfig", "retry_max_elapsed_ms"): ("FinFeedHttpConfig", "retry"),
    ("PyCache", "quote"): ("Cache", "quote_at_index"),
    ("PyCache", "trade"): ("Cache", "trade_at_index"),
    ("PyCache", "bar"): ("Cache", "bar_at_index"),
    ("PyCache", "quotes"): ("Cache", "quotes"),
    ("PyCache", "trades"): ("Cache", "trades"),
    ("PyCache", "bars"): ("Cache", "bars"),
    ("PyCache", "mark_price"): ("Cache", "mark_price"),
    ("PyCache", "mark_prices"): ("Cache", "mark_prices"),
    ("PyCache", "index_price"): ("Cache", "index_price"),
    ("PyCache", "index_prices"): ("Cache", "index_prices"),
    ("PyCache", "funding_rate"): ("Cache", "funding_rate"),
    ("PyCache", "funding_rates"): ("Cache", "funding_rates"),
    ("PyCache", "instrument_status"): ("Cache", "instrument_status"),
    ("PyCache", "instrument_statuses"): ("Cache", "instrument_statuses"),
    ("PyCache", "instrument_close"): ("Cache", "instrument_close"),
    ("PyCache", "order_book"): ("Cache", "order_book"),
}

BANNER_RE = re.compile(r"^-{3,}.*-{3,}$")

# Rust intra-doc link: [`Type::method`] or [`Type`](path)
INTRA_DOC_LINK_RE = re.compile(r"\[`([^`]+)`\](?:\([^)]*\))?")

# Attribute line that closes on this line, allowing a trailing line comment
# (e.g. `#[allow(unused_imports)] // used in template pattern`)
ATTR_END_RE = re.compile(r"\]\s*(?://.*)?$")


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


def collect_source_docs(  # noqa: C901
    src_dir: Path,
) -> dict[tuple[str | None, str], list[str]]:
    """
    Collect doc comments for items in a crate, excluding python/ files.

    Returns {(type_name_or_none, item_name): [doc_line, ...]} where lines exclude the
    ``///`` prefix. Free functions and type definitions use ``None`` as the type_name.
    Methods, public fields, and enum variants use the enclosing type name.

    """
    docs: dict[tuple[str | None, str], list[str]] = {}

    for rs_file in sorted(src_dir.rglob("*.rs")):
        rel = rs_file.relative_to(src_dir)
        if rel.parts[0] == "python":
            continue

        lines = rs_file.read_text(encoding="utf-8").splitlines()
        doc_block: list[str] = []
        in_multiline_attr = False
        current_impl: str | None = None
        current_type: str | None = None
        type_depth = 0
        brace_depth = 0
        impl_depth = 0

        for line in lines:
            stripped = line.strip()

            if in_multiline_attr:
                if ATTR_END_RE.search(stripped):
                    in_multiline_attr = False
                continue

            if stripped.startswith("///"):
                content = stripped[3:].removeprefix(" ")
                doc_block.append(content)
                continue

            if stripped.startswith("#["):
                if rust_attr_continues(stripped):
                    in_multiline_attr = True
                continue

            if stripped == "" or stripped.startswith("//"):
                doc_block = []
                continue

            impl_m = re.match(
                r"\s*impl(?:<[^>]*>)?\s+(?:[\w:]+(?:<[^>]*>)?\s+for\s+)?(\w+)",
                stripped,
            )

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

            type_m = re.match(r"pub\s+(?:struct|enum)\s+(\w+)\s*\{", stripped)
            if type_m:
                current_type = type_m.group(1)
                type_depth = brace_depth
            elif current_type is not None and brace_depth < type_depth:
                current_type = None

            if doc_block:
                is_banner = all(
                    BANNER_RE.match(doc_line) or doc_line == "" for doc_line in doc_block
                )
                fn_m = re.match(
                    r"\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|async\s+)?fn\s+(\w+)",
                    line,
                )

                if fn_m and (
                    current_impl in TYPE_DOC_SOURCES.values() or stripped.startswith("pub")
                ):
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
                    elif current_type is not None:
                        member = re.match(r"(?:pub\s+)?(\w+)\s*[:,]", stripped)
                        if member and not is_banner:
                            docs[(current_type, f"member:{member.group(1)}")] = list(doc_block)

            doc_block = []

    return docs


def transform_doc(
    doc_lines: list[str],
    source_file: str = "",
    fn_name: str = "",
    *,
    strip_errors: bool = False,
) -> list[str]:
    """
    Copy doc lines, dropping sections that do not belong on the Python wrapper.

    Section headers like ``# Errors`` and ``# Safety`` are kept as-is for clippy
    compatibility. The numpydoc transformation happens later in the stub post-processor.

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
    Find py_* functions and Python-exposed methods with their doc comment ranges.

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
            if ATTR_END_RE.search(stripped):
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
            if rust_attr_continues(stripped):
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

        fn_m = re.match(r"\s*(?:pub\s+)?(?:const\s+)?fn\s+(\w+)", line)
        if fn_m and (fn_m.group(1).startswith("py_") or impl_type is not None):
            insert = first_attr_line if first_attr_line is not None else i
            fn_signature = rust_fn_signature(lines, i)

            if doc_start is not None:
                insert = doc_start

            items.append(
                {
                    "fn_name": fn_m.group(1),
                    "fn_line": i,
                    "fn_signature": fn_signature,
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


def rust_attr_continues(line: str) -> bool:
    """
    Return whether a Rust attribute starting on ``line`` continues onto the next line.

    A parameter attribute such as ``#[builder(default)] name: T`` closes its brackets
    before the parameter on the same line.

    """
    return not ATTR_END_RE.search(line) and line.count("[") > line.count("]")


def rust_fn_signature(lines: list[str], start: int) -> str:
    """
    Return a Rust function signature starting at ``start``.
    """
    signature_lines: list[str] = []

    for line in lines[start:]:
        signature_lines.append(line.strip())
        if "{" in line:
            break

    return " ".join(signature_lines)


def rust_fn_returns_result(signature: str) -> bool:
    """
    Return whether a Rust function signature returns a Result-like type.
    """
    match = re.search(r"\)\s*->\s*(.*?)\s*\{", signature)
    if match is None:
        return False

    return_type = match.group(1).split(" where ", maxsplit=1)[0].strip()
    return re.search(r"(?:^|::)\w*Result\b", return_type) is not None


def process_crate(  # noqa: C901
    crate_name: str,
    src_dir: Path,
    *,
    dry_run: bool = False,
    verbose: bool = False,
) -> int:
    """
    Process a single crate, updating PyO3 doc comments.

    Returns number of doc comments updated.

    """
    if verbose:
        print(f"Processing crate: {crate_name}")

    source_docs = collect_source_docs(src_dir)
    if verbose:
        print(f"  Collected {len(source_docs)} source doc comments")

    python_dir = src_dir / "python"
    if not python_dir.is_dir():
        if verbose:
            print("  No python/ directory, skipping")
        return 0

    total_updates = 0
    dependency_docs = {}

    for rs_file in sorted(python_dir.rglob("*.rs")):
        text = rs_file.read_text(encoding="utf-8")
        file_lines = text.splitlines()
        items = parse_pyo3_items(file_lines)
        updates = 0

        for item in reversed(items):
            fn_name = item["fn_name"]
            target = fn_name.removeprefix("py_")

            impl_type = item["impl_type"]
            if not fn_name.startswith("py_") and impl_type not in TYPE_DOC_SOURCES:
                continue

            if item["is_constructor"] and impl_type:
                source_type = TYPE_DOC_SOURCES.get(impl_type, impl_type)
                source_doc = source_docs.get((None, source_type)) or source_docs.get(
                    (source_type, "new"),
                )
            elif impl_type:
                source_type = TYPE_DOC_SOURCES.get(impl_type, impl_type)
                source_key = METHOD_DOC_SOURCES.get((impl_type, target), (source_type, target))
                source_type, source_name = source_key
                if source_type and "::" in source_type:
                    source_crate, source_type = source_type.split("::", maxsplit=1)
                    if source_crate not in dependency_docs:
                        dependency_docs[source_crate] = collect_source_docs(
                            CRATES_DIR / source_crate / "src",
                        )
                    source_doc = dependency_docs[source_crate].get((source_type, source_name))
                    if source_doc is None:
                        source_doc = dependency_docs[source_crate].get(
                            (source_type, f"member:{source_name}"),
                        )
                else:
                    source_doc = source_docs.get(source_key)
                    if source_doc is None and (
                        impl_type in TYPE_DOC_SOURCES or (impl_type, target) in METHOD_DOC_SOURCES
                    ):
                        source_doc = source_docs.get((source_type, f"member:{source_name}"))
                if source_doc and target.startswith("has_") and target[4:] == source_name:
                    source_doc = [f"Returns whether `{source_name}` is set."]
            elif item["in_pymethods"]:
                # Method lost its impl_type (parser brace tracking); skip
                source_doc = None
            else:
                # Standalone pyfunction (no impl block)
                source_doc = source_docs.get((None, target))

            if not source_doc and target == "__repr__":
                body = "\n".join(file_lines[item["fn_line"] :]).lstrip()
                representation = re.match(r'fn __repr__[^\n]*\{\s*"([^"\n]+)"\s*\}', body)
                if representation:
                    source_doc = [
                        f"Returns the constant representation `{representation.group(1)}`.",
                    ]

            if not source_doc:
                continue

            returns_result = rust_fn_returns_result(item["fn_signature"])

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

        for enum in reversed(
            list(
                re.finditer(
                    r"pub enum (\w+) \{\n([\s\S]*?)^\}",
                    "\n".join(file_lines),
                    re.MULTILINE,
                ),
            ),
        ):
            enum_type = TYPE_DOC_SOURCES.get(enum.group(1))
            if enum_type is None:
                continue
            enum_text = enum.group(0)
            for variant in reversed(
                list(re.finditer(r"((?:    ///[^\n]*\n)*)    (\w+),$", enum_text, re.MULTILINE)),
            ):
                source_doc = source_docs.get((enum_type, f"member:{variant.group(2)}"))
                if source_doc is None:
                    continue
                comment = "\n".join(format_as_doc_comment(transform_doc(source_doc), "    ")) + "\n"
                if comment == variant.group(1):
                    continue
                enum_text = (
                    enum_text[: variant.start()]
                    + comment
                    + enum_text[variant.start() + len(variant.group(1)) :]
                )
                updates += 1
            content = "\n".join(file_lines)
            file_lines = (content[: enum.start()] + enum_text + content[enum.end() :]).split("\n")

        if updates:
            rel_path = rs_file.relative_to(ROOT)
            action = "Would update" if dry_run else "Updated"
            print(f"  {action} {updates} doc(s) in {rel_path}")

            if not dry_run:
                rs_file.write_text("\n".join(file_lines) + "\n", encoding="utf-8")

        total_updates += updates

    return total_updates


def main() -> None:
    """
    Parse CLI arguments and generate PyO3 doc comments.
    """
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
    parser.add_argument(
        "--verbose",
        action="store_true",
        help="Show per-crate traversal details",
    )
    args = parser.parse_args()

    crate_dirs = get_crate_src_dirs(args.crate_name)

    if not crate_dirs:
        print("No matching crates found", file=sys.stderr)
        sys.exit(1)

    total = 0
    for name, src_dir in crate_dirs:
        total += process_crate(
            name,
            src_dir,
            dry_run=args.dry_run,
            verbose=args.verbose,
        )

    prefix = "would be " if args.dry_run else ""
    print(f"\nTotal: {total} doc comments {prefix}updated")


if __name__ == "__main__":
    main()
