#!/usr/bin/env python3
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
"""
Test Python spacing, source preservation, and command integration.
"""

from __future__ import annotations

import ast
import io
import runpy
import shutil
import subprocess
import sys
import tokenize
from pathlib import Path

import pytest


# ruff: noqa: D103, PLR2004, S101, S603

ROOT = Path(__file__).resolve().parents[1]
FORMATTER = ROOT / "scripts" / "format-python.py"
FORMAT = runpy.run_path(str(FORMATTER))["_format"]


@pytest.mark.parametrize(
    ("source", "expected"),
    [
        pytest.param(
            "prepare()\nif ready:\n    run()\n",
            "prepare()\n\nif ready:\n    run()\n",
            id="unrelated-if",
        ),
        pytest.param(
            "prepare()\nfor item in items:\n    consume(item)\n",
            "prepare()\n\nfor item in items:\n    consume(item)\n",
            id="unrelated-for",
        ),
        pytest.param(
            "async def run():\n    prepare()\n"
            "    async for item in items:\n        consume(item)\n",
            "async def run():\n    prepare()\n\n"
            "    async for item in items:\n        consume(item)\n",
            id="unrelated-async-for",
        ),
        pytest.param(
            "prepare()\nwhile ready:\n    run()\n",
            "prepare()\n\nwhile ready:\n    run()\n",
            id="unrelated-while",
        ),
        pytest.param(
            'prepare()\nmatch state:\n    case "ready":\n        run()\n',
            'prepare()\n\nmatch state:\n    case "ready":\n        run()\n',
            id="unrelated-match",
        ),
        pytest.param(
            "result = calculate()\n\nif enabled:\n    consume(result)\n",
            "result = calculate()\nif enabled:\n    consume(result)\n",
            id="body-continuation-removes-gap",
        ),
        pytest.param(
            "result = calculate()\n\nif (\n    enabled\n):\n    consume(result)\n",
            "result = calculate()\nif (\n    enabled\n):\n    consume(result)\n",
            id="wrapped-header-body-continuation",
        ),
        pytest.param(
            "result = calculate()\nif enabled:\n    prepare()\n    consume(result)\n",
            "result = calculate()\n\nif enabled:\n    prepare()\n    consume(result)\n",
            id="later-body-reference-does-not-continue",
        ),
        pytest.param(
            "result = calculate()\nif enabled:\n    consume(\n        result,\n    )\n",
            "result = calculate()\n\nif enabled:\n    consume(\n        result,\n    )\n",
            id="wrapped-body-reference-does-not-continue",
        ),
        pytest.param(
            'message = "ready"\nif ready:\n    run()\n',
            'message = "ready"\n\nif ready:\n    run()\n',
            id="string-is-not-a-reference",
        ),
        pytest.param(
            "prepare()\nif enabled:\n    prepare()\n",
            "prepare()\n\nif enabled:\n    prepare()\n",
            id="call-name-is-not-a-reference",
        ),
        pytest.param(
            "item = load()\nfor item in values:\n    consume(item)\n",
            "item = load()\n\nfor item in values:\n    consume(item)\n",
            id="loop-binding-shadows-name",
        ),
        pytest.param(
            "item = load()\nif enabled:\n    consume([item for item in values])\n",
            "item = load()\n\nif enabled:\n    consume([item for item in values])\n",
            id="comprehension-binding-shadows-name",
        ),
        pytest.param(
            "item = load()\nif enabled:\n    consume(lambda item: item)\n",
            "item = load()\n\nif enabled:\n    consume(lambda item: item)\n",
            id="lambda-binding-shadows-name",
        ),
        pytest.param(
            "item = load()\nmatch value:\n    case item:\n        consume(item)\n",
            "item = load()\n\nmatch value:\n    case item:\n        consume(item)\n",
            id="match-binding-shadows-name",
        ),
        pytest.param(
            "item = load()\nif (item := other()) and item:\n    run()\n",
            "item = load()\n\nif (item := other()) and item:\n    run()\n",
            id="assignment-expression-shadows-name",
        ),
        pytest.param(
            "self.prepare()\nif self.ready:\n    self.run()\n",
            "self.prepare()\n\nif self.ready:\n    self.run()\n",
            id="self-alone-does-not-connect-statements",
        ),
        pytest.param(
            "prepare()\n# Choose the active path\nif ready:\n    run()\n",
            "prepare()\n\n# Choose the active path\nif ready:\n    run()\n",
            id="separator-above-attached-comment",
        ),
        pytest.param(
            "result = calculate()\n# Explain the branch\nif enabled:\n    consume(result)\n",
            "result = calculate()\n\n# Explain the branch\nif enabled:\n    consume(result)\n",
            id="attached-comment-separates-body-continuation",
        ),
        pytest.param(
            "if enabled:\n    result = compute()\n    if ready:\n        consume(result)\n",
            "if enabled:\n    result = compute()\n    if ready:\n        consume(result)\n",
            id="nested-body-continuation",
        ),
    ],
)
def test_control_flow_spacing(source: str, expected: str) -> None:
    _assert_spacing(source, expected)


@pytest.mark.parametrize("keyword", ["if", "while"])
@pytest.mark.parametrize("separator", ["", "\n"])
def test_header_binding_does_not_continue_previous_value(keyword: str, separator: str) -> None:
    source = f"result = load()\n{separator}{keyword} result := other():\n    consume(result)\n"
    expected = f"result = load()\n\n{keyword} result := other():\n    consume(result)\n"
    _assert_spacing(source, expected)


def test_header_can_read_previous_value_before_rebinding() -> None:
    source = "result = load()\n\nif result := transform(result):\n    consume(result)\n"
    _assert_spacing(source, source)


@pytest.mark.parametrize(
    "previous",
    [
        "if first:\n    total = calculate()\n",
        "for item in items:\n    total = calculate(item)\n",
        "while active:\n    total = calculate()\n",
        "match state:\n    case _: total = calculate()\n",
        "with context():\n    total = calculate()\n",
        "try:\n    total = calculate()\nexcept ValueError:\n    total = 0\n",
    ],
)
@pytest.mark.parametrize("separator", ["", "\n"])
def test_continuation_does_not_cross_previous_block(previous: str, separator: str) -> None:
    current = "if enabled:\n    consume(total)\n"
    _assert_spacing(previous + separator + current, previous + "\n" + current)


@pytest.mark.parametrize("separator", ["", "\n"])
@pytest.mark.parametrize("attached", ["", "# Explain the next branch\n"])
def test_separator_follows_previous_block_trailing_comment(separator: str, attached: str) -> None:
    previous = "if enabled:\n    run()\n    # Complete the previous block\n"
    current = "if ready:\n    consume()\n"
    _assert_spacing(previous + separator + attached + current, previous + "\n" + attached + current)


def test_nested_separator_follows_previous_block_trailing_comment() -> None:
    previous = "def run():\n    if enabled:\n        consume()\n        # Complete this branch\n"
    current = "    if ready:\n        finish()\n"
    _assert_spacing(previous + current, previous + "\n" + current)


def test_annotation_names_do_not_establish_continuation() -> None:
    source = "result: dict[str, int] = {}\n\nif enabled:\n    current: str | None = None\n"
    _assert_spacing(source, source)


def test_annotated_value_can_continue_previous_statement() -> None:
    previous = "result: int = calculate()\n"
    current = "if enabled:\n    current: int = consume(result)\n"
    _assert_spacing(previous + "\n" + current, previous + current)


@pytest.mark.parametrize(
    ("previous", "statement"),
    [
        ("engine.add_venue(venue)\n", "engine.run()"),
        ("config.value = load()\n", "config.value = other()"),
    ],
)
def test_shared_named_receiver_establishes_continuation(previous: str, statement: str) -> None:
    current = f"if ready:\n    {statement}\n"
    _assert_spacing(previous + "\n" + current, previous + current)


def test_explicit_continuation_preserves_terminating_newline() -> None:
    source = "value = 1 " + chr(92) + "\n\nif ready:\n    consume(value)\n"
    _assert_spacing(source, source)


@pytest.mark.parametrize("directive", ["# fmt: skip", "# noqa: F821 # fmt: skip"])
def test_combined_skip_comments_preserve_statement(directive: str) -> None:
    source = "prepare()\nvalue = Order(\n  first,\n  second,\n)  " + directive + "\nfinish()\n"
    _assert_spacing(source, source)
    assert _format_ruff(source) == source


@pytest.mark.parametrize("comment", ["# note # fmt: on", "# noqa: E501 # fmt: on"])
def test_combined_on_comment_does_not_end_disabled_region(comment: str) -> None:
    source = (
        "prepare()\n# fmt: off\nvalue = [\n  1,\n]\n"
        + comment
        + "\nfinish()\nif ready:\n    run()\n"
    )
    _assert_spacing(source, source)
    assert _format_ruff(source) == source


@pytest.mark.parametrize("comment", ["# note # fmt: off", "# fmt: off # noqa"])
def test_combined_off_comment_does_not_disable_formatting(comment: str) -> None:
    source = "prepare()\n" + comment + "\nfinish()\nif ready:\n    run()\n"
    expected = source.replace("\nif ready:", "\n\nif ready:")
    _assert_spacing(source, expected)
    probe = source.replace("finish()", "finish( )")
    assert _format_ruff(probe) == source


@pytest.mark.parametrize(
    "tail",
    [
        "prepare()\nif ready:\n    run(  )\n",
        "    prepare()\n    if ready:\n        run(  )\n",
    ],
)
def test_nested_on_comment_does_not_end_outer_disabled_region(tail: str) -> None:
    source = "# fmt: off\nif enabled:\n    x = 1\n    # fmt: on\n" + tail
    _assert_spacing(source, source)
    assert _format_ruff(source) == source


def test_matching_on_comment_ends_disabled_region() -> None:
    source = "# fmt: off\nif enabled:\n    x = 1\n# fmt: on\nprepare()\nif ready:\n    run(  )\n"
    expected = source.replace("\nif ready:", "\n\nif ready:")
    _assert_spacing(source, expected)
    assert _format_ruff(source) == source.replace("run(  )", "run()")


@pytest.mark.parametrize("receiver", ["self", "cls"])
@pytest.mark.parametrize("separator", ["", "\n"])
def test_attribute_overwrite_does_not_read_previous_value(receiver: str, separator: str) -> None:
    previous = f"{receiver}.result = load()\n"
    current = f"if ready:\n    {receiver}.result = other()\n"
    _assert_spacing(previous + separator + current, previous + "\n" + current)


@pytest.mark.parametrize(
    "statement",
    ["self.result += 1", "self.result = transform(self.result)", "consume(self.result)"],
)
def test_attribute_read_continues_previous_value(statement: str) -> None:
    previous = "self.result = load()\n"
    current = f"if ready:\n    {statement}\n"
    _assert_spacing(previous + "\n" + current, previous + current)


def test_match_guard_binding_does_not_continue_previous_value() -> None:
    source = (
        "result = load()\n\nmatch value:\n"
        "    case _ if (result := other()):\n        consume(result)\n"
    )
    _assert_spacing(source, source)


@pytest.mark.parametrize(
    "condition",
    [
        "any((result := other(item)) for item in items)",
        "[(result := other(item)) for item in items]",
        "{(result := other(item)) for item in items}",
        "{item: (result := other(item)) for item in items}",
    ],
)
def test_comprehension_walrus_binding_escapes_to_body(condition: str) -> None:
    source = f"result = load()\n\nif {condition}:\n    consume(result)\n"
    _assert_spacing(source, source)


@pytest.mark.parametrize(
    "construction",
    [
        "[\n    first,\n    second,\n]",
        "(\n    first,\n    second,\n)",
        "{\n    first,\n    second,\n}",
        '{\n    "first": first,\n    "second": second,\n}',
        "Order(\n    first,\n    second,\n)",
        "model.Order(\n    first,\n    second,\n)",
        "_Order(\n    first,\n    second,\n)",
        "UUID(\n    first,\n    second,\n)",
    ],
)
def test_multiline_construction_spacing(construction: str) -> None:
    source = f"prepare()\nvalue = {construction}\nconsume(value)\n"
    expected = f"prepare()\n\nvalue = {construction}\n\nconsume(value)\n"
    _assert_spacing(source, expected)


@pytest.mark.parametrize(
    "statement",
    [
        "value: Order = Order(\n    first,\n    second,\n)",
        "return Order(\n    first,\n    second,\n)",
        "yield Order(\n    first,\n    second,\n)",
        "value = await Order(\n    first,\n    second,\n)",
    ],
)
def test_construction_statement_forms(statement: str) -> None:
    source = "async def run():\n" + "    prepare()\n" + _indent(statement) + "    finish()\n"
    expected = "async def run():\n    prepare()\n\n" + _indent(statement) + "\n    finish()\n"
    _assert_spacing(source, expected)


@pytest.mark.parametrize(
    "source",
    [
        "first = Order()\nsecond = Order()\n",
        "prepare()\nvalue = calculate(\n    first,\n    second,\n)\nconsume(value)\n",
        "prepare()\nvalue = Order(\n    first,\n    second,\n).build()\nconsume(value)\n",
        "prepare()\nconsume(Order(\n    first,\n    second,\n))\nfinish()\n",
        "values = [\n    value\n    for value in items\n    if value.ready\n]\nconsume(values)\n",
        "ready = check()\nif ready:\n    run()\n",
        "ready = check()\n\nif ready:\n    run()\n",
        "self.ready = check()\nif self.ready:\n    run()\n",
        "result = compute()\nif ready:\n    consume(result)\n",
        "result = compute()\n\n# Keep this explanation attached\nif ready:\n    consume(result)\n",
        (
            "def run():\n    if ready:\n        first()\n"
            "    elif enabled:\n        second()\n    else:\n        third()\n"
        ),
        'def run():\n    """\n    Run it.\n    """\n    if ready:\n        run()\n',
        'def run():\n    """Run it."""\n    if ready:\n        run()\n',
        'message = """\nprepare()\nif ready:\n    run()\n\n\u2028\n"""\n',
        "prepare()\n# fmt: off\nif ready:\n    run()\n# fmt: on\n",
        "prepare()\n# fmt: off\nif ready:\n    run()\n",
        "prepare()\nif ready:  # fmt: skip\n    run()\n",
        "prepare()\nvalue = Order(\n    first,\n    second,\n)  # fmt: skip\nfinish()\n",
        "prepare()\n\nvalue = [\n    first,\n    second,\n]\n\nfinish()\n",
        "value = [\n    first,\n    second,\n]\n",
        "def run():\n    prepare()\n\n\ndef other():\n    prepare()\n",
        "prepare(); run()\n",
        "def first():\n    pass\ndef second():\n    pass\n",
        "item = load()\nif (item := transform(item)):\n    run()\n",
        "",
    ],
)
def test_preserves_existing_layout(source: str) -> None:
    _assert_spacing(source, source)


def test_crlf_line_endings() -> None:
    _assert_spacing(
        "prepare()\r\nif ready:\r\n    run()\r\n",
        "prepare()\r\n\r\nif ready:\r\n    run()\r\n",
    )


def test_diagnostics_use_original_line_numbers() -> None:
    formatted, diagnostics = FORMAT("prepare()\nif ready:\n    run()\n")
    assert formatted == "prepare()\n\nif ready:\n    run()\n"
    assert diagnostics == [(2, "Missing blank line above `if`")]


def test_cli_check_fix_and_recheck(tmp_path: Path) -> None:
    path = tmp_path / "with spaces.py"
    source = "prepare()\nif ready:\n    run()\n"
    expected = "prepare()\n\nif ready:\n    run()\n"
    path.write_text(source, encoding="utf-8")

    checked = _run("--check", str(path))
    before_fix = path.read_text(encoding="utf-8")
    fixed = _run("--fix", str(path))
    after_fix = path.read_text(encoding="utf-8")
    rechecked = _run(str(path))

    assert checked.returncode == 1
    assert checked.stdout == (
        f"{path}:2: Missing blank line above `if`\nRequired spacing changes: 1 (files: 1)\n"
    )
    assert checked.stderr == ""
    assert before_fix == source
    assert fixed.returncode == 0
    assert fixed.stdout == "Applied spacing changes: 1 (files: 1)\n"
    assert fixed.stderr == ""
    assert after_fix == expected
    assert rechecked.returncode == 0
    assert rechecked.stdout == "Required spacing changes: 0 (files: 0)\n"
    assert rechecked.stderr == ""


@pytest.mark.parametrize("invalid", ["def broken(:\n", "# coding: unknown-encoding\n"])
def test_validates_all_files_before_writing(tmp_path: Path, invalid: str) -> None:
    valid_path = tmp_path / "a_valid.py"
    invalid_path = tmp_path / "z_invalid.py"
    source = "prepare()\nif ready:\n    run()\n"
    valid_path.write_text(source, encoding="utf-8")
    invalid_path.write_text(invalid, encoding="utf-8")

    result = _run("--fix", str(tmp_path))

    assert result.returncode == 2
    assert result.stdout == ""
    assert "Python formatting failed:" in result.stderr
    assert valid_path.read_text(encoding="utf-8") == source
    assert invalid_path.read_text(encoding="utf-8") == invalid


def test_hook_formats_only_selected_files(tmp_path: Path, monkeypatch: pytest.MonkeyPatch) -> None:
    selected = tmp_path / "selected.py"
    untouched = tmp_path / "untouched.py"
    source = "prepare()\nif ready:\n    run()\n"
    selected.write_text(source, encoding="utf-8")
    untouched.write_text(source, encoding="utf-8")
    bash = shutil.which("bash")
    if bash is None:
        pytest.fail("Bash is required to test the formatting hook")
    monkeypatch.setenv("VIRTUAL_ENV", str(tmp_path / "another-environment"))

    result = subprocess.run(
        [
            bash,
            str(ROOT / ".pre-commit-hooks" / "check_formatting_py.sh"),
            "--fix",
            str(selected),
        ],
        cwd=tmp_path,
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode == 0
    assert result.stdout == "Applied spacing changes: 1 (files: 1)\n"
    assert result.stderr == ""
    assert selected.read_text(encoding="utf-8") == source.replace("\nif ready:", "\n\nif ready:")
    assert untouched.read_text(encoding="utf-8") == source


def test_missing_path_prevents_writes(tmp_path: Path) -> None:
    selected = tmp_path / "selected.py"
    missing = tmp_path / "missing.py"
    source = "prepare()\nif ready:\n    run()\n"
    selected.write_text(source, encoding="utf-8")

    result = _run("--fix", str(selected), str(missing))

    assert result.returncode == 2
    assert result.stdout == ""
    assert str(missing) in result.stderr
    assert selected.read_text(encoding="utf-8") == source


def test_lone_cr_failure_leaves_all_selected_files_unchanged(tmp_path: Path) -> None:
    valid = tmp_path / "a_valid.py"
    invalid = tmp_path / "z_lone_cr.py"
    source = b"prepare()\nif ready:\n    run()\n"
    lone_cr = b'a = 1\rb = 2\rc = 3\rif ready:\n    run()\ntext = """\nfirst\nsecond\n"""\n'
    valid.write_bytes(source)
    invalid.write_bytes(lone_cr)

    result = _run("--fix", str(tmp_path))

    assert result.returncode == 2
    assert result.stdout == ""
    assert result.stderr == (
        f"Python formatting failed: {invalid}: "
        "lone CR line endings are unsupported; convert to LF or CRLF\n"
    )
    assert valid.read_bytes() == source
    assert invalid.read_bytes() == lone_cr


@pytest.mark.parametrize("parent", ["generated", "stubs"])
def test_checkout_parent_does_not_exclude_source(tmp_path: Path, parent: str) -> None:
    checkout = tmp_path / parent / "checkout"
    (checkout / "scripts").mkdir(parents=True)
    (checkout / "python").mkdir()
    copied_formatter = checkout / "scripts" / FORMATTER.name
    shutil.copy2(FORMATTER, copied_formatter)
    shutil.copy2(ROOT / "python" / "pyproject.toml", checkout / "python" / "pyproject.toml")
    selected = checkout / "example.py"
    source = "prepare()\nif ready:\n    run()\n"
    selected.write_text(source, encoding="utf-8")

    result = subprocess.run(
        [sys.executable, "-B", str(copied_formatter), "--fix", str(selected)],
        text=True,
        capture_output=True,
        check=False,
    )

    assert result.returncode == 0
    assert result.stdout == "Applied spacing changes: 1 (files: 1)\n"
    assert result.stderr == ""
    assert selected.read_text(encoding="utf-8") == source.replace("\nif ready:", "\n\nif ready:")


def test_symlink_is_not_formatted(tmp_path: Path) -> None:
    target = tmp_path / "target.py"
    link = tmp_path / "link.py"
    source = "prepare()\nif ready:\n    run()\n"
    target.write_text(source, encoding="utf-8")
    try:
        link.symlink_to(target)
    except OSError:
        pytest.skip("Symlinks are unavailable on this host")

    result = _run("--fix", str(link))

    assert result.returncode == 0
    assert result.stdout == "Applied spacing changes: 0 (files: 0)\n"
    assert result.stderr == ""
    assert target.read_text(encoding="utf-8") == source
    assert link.is_symlink()


@pytest.mark.parametrize("encoding", ["utf-8-sig", "latin-1"])
def test_preserves_source_encoding(tmp_path: Path, encoding: str) -> None:
    path = tmp_path / "encoded.py"
    source = f'# coding: {encoding}\nname = "\u00e9"\nif ready:\n    run()\n'
    expected = source.replace("\nif ready:", "\n\nif ready:")
    path.write_bytes(source.encode(encoding))

    result = _run("--fix", str(path))

    assert result.returncode == 0
    assert path.read_bytes() == expected.encode(encoding)


def test_directory_selection_skips_generated_and_environment_files(tmp_path: Path) -> None:
    source = "prepare()\nif ready:\n    run()\n"
    (tmp_path / ".git").mkdir()
    (tmp_path / ".gitignore").write_text(".venv/\ntarget/\nignored.py\n", encoding="utf-8")
    for directory in (".venv", "generated", "patches", "stubs", "target"):
        path = tmp_path / directory / "unchanged.py"
        path.parent.mkdir()
        path.write_text(source, encoding="utf-8")
    stub = tmp_path / "generated.pyi"
    stub.write_text(source, encoding="utf-8")
    ignored = tmp_path / "ignored.py"
    ignored.write_text(source, encoding="utf-8")
    selected = tmp_path / "example.py"
    selected.write_text(source, encoding="utf-8")

    result = _run("--fix", str(tmp_path), str(selected))

    assert result.returncode == 0
    assert result.stdout == "Applied spacing changes: 1 (files: 1)\n"
    assert selected.read_text(encoding="utf-8") == source.replace("\nif ready:", "\n\nif ready:")
    assert stub.read_text(encoding="utf-8") == source
    assert ignored.read_text(encoding="utf-8") == source
    for directory in (".venv", "generated", "patches", "stubs", "target"):
        assert (tmp_path / directory / "unchanged.py").read_text(encoding="utf-8") == source


@pytest.mark.parametrize(
    ("source", "changes"),
    [
        (
            "prepare()\nvalue = Order(first, second,)\nconsume(value)\n",
            2,
        ),
        ("prepare()\nvalue = [\n    first,\n    second,\n]\nconsume(value)\n", 2),
        ("result = calculate()\n\nif (enabled and also_enabled):\n    consume(result)\n", 1),
        ("prepare()\nif enabled:\n    consume()\n", 1),
    ],
)
def test_ruff_preserves_spacing(source: str, changes: int) -> None:
    layout = _format_ruff(source)
    formatted, diagnostics = FORMAT(layout)
    repeated = _format_ruff(formatted)

    assert repeated == formatted
    assert len(diagnostics) == changes
    assert FORMAT(formatted) == (formatted, [])


def _assert_spacing(source: str, expected: str) -> None:
    formatted, diagnostics = FORMAT(source)

    assert formatted == expected
    assert bool(diagnostics) == (source != expected)
    assert _tokens(formatted) == _tokens(source)
    assert ast.dump(ast.parse(formatted)) == ast.dump(ast.parse(source))
    assert FORMAT(formatted) == (formatted, [])


def _tokens(source: str) -> list[tuple[int, str]]:
    return [
        (token.type, token.string)
        for token in tokenize.generate_tokens(io.StringIO(source).readline)
        if token.type != tokenize.NL
    ]


def _indent(source: str) -> str:
    return "".join(f"    {line}\n" for line in source.splitlines())


def _run(*args: str) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        [sys.executable, "-B", str(FORMATTER), *args],
        text=True,
        capture_output=True,
        check=False,
    )


def _format_ruff(source: str) -> str:
    return subprocess.run(
        [
            sys.executable,
            "-m",
            "ruff",
            "format",
            "--config",
            str(ROOT / "python" / "pyproject.toml"),
            "-",
        ],
        input=source,
        text=True,
        capture_output=True,
        check=True,
    ).stdout
