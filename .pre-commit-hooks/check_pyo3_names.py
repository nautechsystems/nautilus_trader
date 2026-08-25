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
Check that PyO3 functions and classes expose explicit public names.
"""

from __future__ import annotations

import re
import shutil
import subprocess
import sys
from pathlib import Path


# These custom-data methods use PyArrow objects, so `_py` is part of their public protocol
PUBLIC_SUFFIX_NAMES = frozenset(
    {
        "decode_record_batch_py",
        "encode_record_batch_py",
    },
)
PYMETHOD_MACRO_PATH = "persistence/macros/src/custom.rs"
RAW_STRING_PATTERN = re.compile(r'(?:br|rb|r)(?P<hashes>#+)?"')


def _mask_non_code(source: str) -> str:  # noqa: C901
    masked = list(source)
    length = len(source)
    i = 0

    def blank(start: int, end: int) -> None:
        masked[start:end] = ["\n" if char == "\n" else " " for char in source[start:end]]

    while i < length:
        if source.startswith("//", i):
            end = source.find("\n", i)
            end = length if end == -1 else end
            blank(i, end)
            i = end
            continue

        if source.startswith("/*", i):
            start = i
            depth = 1
            i += 2
            while i < length and depth:
                if source.startswith("/*", i):
                    depth += 1
                    i += 2
                elif source.startswith("*/", i):
                    depth -= 1
                    i += 2
                else:
                    i += 1
            blank(start, i)
            continue

        raw_match = RAW_STRING_PATTERN.match(source, i) if source[i] in {"b", "r"} else None
        if raw_match is not None:
            start = i
            hashes = raw_match.group("hashes") or ""
            i = raw_match.end()
            end_token = f'"{hashes}'
            end = source.find(end_token, i)
            i = length if end == -1 else end + len(end_token)
            blank(start, i)
            continue

        string_start = i + 1 if source[i] == "b" and source[i + 1 : i + 2] == '"' else i
        if source[i] == '"' or string_start != i:
            start = i
            i = string_start + 1
            while i < length:
                if source[i] == "\\":
                    i += 2
                elif source[i] == '"':
                    i += 1
                    break
                else:
                    i += 1
            blank(start, min(i, length))
            continue

        if source[i] == "'":
            char_end = i + 2
            if i + 1 < length and source[i + 1] == "\\":
                char_end += 1
            if char_end < length and source[char_end] == "'":
                blank(i, char_end + 1)
                i = char_end + 1
                continue

        i += 1

    return "".join(masked)


def _matching_delimiter(source: str, start: int, opening: str, closing: str) -> int | None:
    depth = 0
    for index in range(start, len(source)):
        if source[index] == opening:
            depth += 1
        elif source[index] == closing:
            depth -= 1
            if depth == 0:
                return index
    return None


def _attributes(masked: str, source: str, start: int, end: int) -> list[tuple[int, int, str, str]]:
    attributes = []
    index = start
    while True:
        index = masked.find("#[", index, end)
        if index == -1:
            break
        close = _matching_delimiter(masked, index + 1, "[", "]")
        if close is None or close >= end:
            break
        attributes.append(
            (index, close + 1, masked[index : close + 1], source[index : close + 1]),
        )
        index = close + 1
    return attributes


def _is_attribute(masked_attribute: str, name: str) -> bool:
    return re.search(rf"\b{name}\b", masked_attribute) is not None


def _is_python_custom_data(masked_attribute: str) -> bool:
    return (
        _is_attribute(masked_attribute, "custom_data")
        and re.search(
            r"\b(?:pyo3|python)\b",
            masked_attribute,
        )
        is not None
    )


def _pymethod_ranges(masked: str, source: str) -> list[tuple[int, int]]:
    ranges = []
    for _, attribute_end, masked_attribute, _ in _attributes(
        masked,
        source,
        0,
        len(source),
    ):
        if not _is_attribute(masked_attribute, "pymethods"):
            continue
        impl_match = re.search(r"\bimpl\b", masked[attribute_end : attribute_end + 2_000])
        if impl_match is None:
            continue
        impl_start = attribute_end + impl_match.start()
        body_start = masked.find("{", impl_start, impl_start + 2_000)
        if body_start == -1:
            continue
        body_end = _matching_delimiter(masked, body_start, "{", "}")
        if body_end is not None:
            ranges.append((body_start, body_end))
    return ranges


def _item_header(masked: str, source: str, position: int) -> list[tuple[int, int, str, str]]:
    start = max(
        masked.rfind("}", 0, position),
        masked.rfind("{", 0, position),
        masked.rfind(";", 0, position),
    )
    return _attributes(masked, source, start + 1, position)


def _public_name(attributes: list[tuple[int, int, str, str]]) -> str | None:
    for _, _, masked_attribute, attribute in attributes:
        if not (
            _is_attribute(masked_attribute, "pyo3")
            or _is_attribute(masked_attribute, "pyfunction")
            or _is_attribute(masked_attribute, "pyclass")
        ):
            continue
        name_match = re.search(r'\bname\s*=\s*"([^"]+)"', attribute)
        if name_match is not None:
            return name_match.group(1)
    return None


def _line_number(source: str, position: int) -> int:
    return source.count("\n", 0, position) + 1


def _check_file(path: Path) -> list[str]:
    source = path.read_text()
    masked = _mask_non_code(source)
    pymethod_ranges = _pymethod_ranges(masked, source)
    violations = []

    for match in re.finditer(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*_py)\b", masked):
        rust_name = match.group(1)
        attributes = _item_header(masked, source, match.start())
        if not (
            any(
                _is_attribute(masked_attribute, "pyfunction")
                for _, _, masked_attribute, _ in attributes
            )
            or any(start < match.start() < end for start, end in pymethod_ranges)
            or path.as_posix().endswith(PYMETHOD_MACRO_PATH)
        ):
            continue

        public_name = _public_name(attributes)
        if rust_name in PUBLIC_SUFFIX_NAMES:
            if public_name not in (None, rust_name):
                violations.append(
                    f"{path}:{_line_number(source, match.start())}: {rust_name} must remain "
                    f"the public protocol name, was {public_name!r}",
                )
            continue

        expected = rust_name.removesuffix("_py").removeprefix("py_")
        if public_name != expected:
            violations.append(
                f"{path}:{_line_number(source, match.start())}: {rust_name} must use "
                f'#[pyo3(name = "{expected}")], was {public_name!r}',
            )

    for match in re.finditer(r"\b(?:struct|enum)\s+(Py[A-Z][A-Za-z0-9_]*)\b", masked):
        rust_name = match.group(1)
        attributes = _item_header(masked, source, match.start())
        is_pyclass = any(
            _is_attribute(masked_attribute, "pyclass") for _, _, masked_attribute, _ in attributes
        )
        is_custom_data = any(
            _is_python_custom_data(masked_attribute) for _, _, masked_attribute, _ in attributes
        )
        if not is_pyclass and not is_custom_data:
            continue

        public_name = _public_name(attributes)
        expected = rust_name.removeprefix("Py")
        if is_custom_data and public_name is None:
            violations.append(
                f"{path}:{_line_number(source, match.start())}: {rust_name} generated by "
                "#[custom_data(...)] must not use the Rust-only Py prefix",
            )
            continue
        if public_name != expected:
            violations.append(
                f"{path}:{_line_number(source, match.start())}: {rust_name} must use "
                f'#[pyclass(name = "{expected}")], was {public_name!r}',
            )

    return violations


def _candidate_paths(root: Path) -> list[Path]:
    rg_path = shutil.which("rg")
    if rg_path is None:
        raise RuntimeError("ripgrep is required")

    result = subprocess.run(
        [
            rg_path,
            "-l",
            r"\bfn\s+[A-Za-z_][A-Za-z0-9_]*_py\b|\b(?:struct|enum)\s+Py[A-Z][A-Za-z0-9_]*\b",
            "--glob",
            "*.rs",
            "--null",
            str(root),
        ],
        check=False,
        capture_output=True,
    )
    if result.returncode not in {0, 1}:
        sys.stderr.buffer.write(result.stderr)
        raise SystemExit(result.returncode)
    return sorted(Path(path) for path in result.stdout.decode().split("\0") if path)


def main() -> int:
    """
    Check the candidate Rust files and return 1 when any violations are found.
    """
    root = Path(sys.argv[1]) if len(sys.argv) > 1 else Path("crates")
    violations = [violation for path in _candidate_paths(root) for violation in _check_file(path)]
    if violations:
        print("PyO3 public name violations:")
        for violation in violations:
            print(f"  {violation}")
        return 1

    print("All PyO3 public names are explicit or allowlisted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
