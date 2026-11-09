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
Check or fix Python statement spacing after Ruff formats the source.
"""

from __future__ import annotations

import argparse
import ast
import copy
import io
import re
import subprocess
import sys
import tokenize
from itertools import pairwise
from pathlib import Path
from typing import TYPE_CHECKING


if TYPE_CHECKING:
    from collections.abc import Iterator


CONTROL_FLOW = (ast.If, ast.Match, ast.For, ast.AsyncFor, ast.While)
CONTROL_BLOCKS = (*CONTROL_FLOW, ast.With, ast.AsyncWith, ast.Try, ast.TryStar)
DEFINITIONS = (ast.FunctionDef, ast.AsyncFunctionDef, ast.ClassDef)


def _main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    mode = parser.add_mutually_exclusive_group()
    mode.add_argument("--check", dest="fix", action="store_false", help="Report spacing violations")
    mode.add_argument(
        "--fix",
        action="store_true",
        help="Apply spacing fixes without staging changes",
    )
    parser.set_defaults(fix=False)
    parser.add_argument("paths", nargs="+", type=Path, help="Python files or directories")
    args = parser.parse_args()

    pending = []
    try:
        for path in _paths(args.paths):
            original = path.read_bytes()
            encoding, _ = tokenize.detect_encoding(io.BytesIO(original).readline)
            source, diagnostics = _format(original.decode(encoding), filename=str(path))
            if diagnostics:
                pending.append((path, source.encode(encoding), diagnostics))
    except (OSError, UnicodeError, SyntaxError, tokenize.TokenError, ValueError) as e:
        sys.stderr.write(f"Python formatting failed: {e}\n")
        return 2

    for path, content, diagnostics in pending:
        if args.fix:
            try:
                path.write_bytes(content)
            except OSError as e:
                sys.stderr.write(f"Python formatting failed: {e}\n")
                return 2
        else:
            for line, message in diagnostics:
                sys.stdout.write(f"{path}:{line}: {message}\n")

    count = sum(len(diagnostics) for _, _, diagnostics in pending)
    action = "Applied" if args.fix else "Required"
    sys.stdout.write(f"{action} spacing changes: {count} (files: {len(pending)})\n")
    return int(bool(pending) and not args.fix)


def _paths(paths: list[Path]) -> Iterator[Path]:
    for path in paths:
        if not path.exists() and not path.is_symlink():
            raise FileNotFoundError(f"File not found: {path}")
    root = Path(__file__).resolve().parents[1]
    config = root / "python" / "pyproject.toml"
    if not config.is_file():
        config = root / "pyproject.toml"

    # Reuse Ruff's discovery so exclusions and Git ignore rules agree with Ruff format
    result = subprocess.run(  # noqa: S603
        [
            sys.executable,
            "-m",
            "ruff",
            "check",
            "--show-files",
            "--force-exclude",
            "--config",
            str(config),
            "--",
            *map(str, paths),
        ],
        capture_output=True,
        text=True,
        check=False,
    )

    if result.returncode:
        raise ValueError(result.stderr.strip())

    for name in sorted(set(result.stdout.splitlines())):
        path = Path(name)
        parts = path.relative_to(root).parts if path.is_relative_to(root) else path.parts
        if (
            path.suffix == ".py"
            and not {"generated", "stubs"}.intersection(parts)
            and not path.is_symlink()
        ):
            yield path


def _format(source: str, *, filename: str = "<unknown>") -> tuple[str, list[tuple[int, str]]]:
    if "\r" in source.replace("\r\n", ""):
        raise ValueError(f"{filename}: lone CR line endings are unsupported; convert to LF or CRLF")
    tree = ast.parse(source, filename=filename)
    lines = io.StringIO(source).readlines()
    comments = {
        token.start[0]: token
        for token in tokenize.generate_tokens(io.StringIO(source).readline)
        if token.type == tokenize.COMMENT
    }
    statements = [node for node in ast.walk(tree) if isinstance(node, ast.stmt)]
    disabled = _disabled_lines(comments, statements, len(lines))
    edits = []
    diagnostics = []

    for node in ast.walk(tree):
        for _, value in ast.iter_fields(node):
            if not isinstance(value, list) or not all(isinstance(item, ast.stmt) for item in value):
                continue

            for previous, current in pairwise(value):
                if (
                    previous is value[0]
                    and isinstance(node, (ast.Module, *DEFINITIONS))
                    and isinstance(previous, ast.Expr)
                    and isinstance(previous.value, ast.Constant)
                    and isinstance(previous.value.value, str)
                ):
                    continue
                edit = _spacing(previous, current, lines, disabled)
                if edit is not None:
                    start, end, replacement, message = edit
                    edits.append((start, end, replacement))
                    diagnostics.append((current.lineno, message))

    for start, end, replacement in sorted(edits, reverse=True):
        lines[start:end] = replacement
    return "".join(lines), sorted(diagnostics)


def _disabled_lines(
    comments: dict[int, tokenize.TokenInfo],
    statements: list[ast.stmt],
    line_count: int,
) -> set[int]:
    disabled = set()
    start = None

    # Ruff permits combined directives for skip only
    for line, comment in sorted(comments.items()):
        directive = re.fullmatch(r"#\s*fmt:\s*(off|on)\s*", comment.string) or re.search(
            r"#\s*fmt:\s*(skip)\b",
            comment.string,
        )

        if directive is None:
            continue

        match directive[1]:
            case "off":
                if start is None:
                    start = line
            case "on":
                if start is not None and comment.start[1] == comments[start].start[1]:
                    disabled.update(range(start, line + 1))
                    start = None
            case "skip":
                containing = [stmt for stmt in statements if stmt.lineno <= line <= stmt.end_lineno]
                if containing:
                    stmt = min(containing, key=lambda stmt: stmt.end_lineno - stmt.lineno)
                    disabled.update(range(stmt.lineno, stmt.end_lineno + 1))

    if start is not None:
        disabled.update(range(start, line_count + 1))
    return disabled


# Keep rule precedence together: body continuation overrides construction spacing
def _spacing(  # noqa: C901, PLR0911
    previous: ast.stmt,
    current: ast.stmt,
    lines: list[str],
    disabled: set[int],
) -> tuple[int, int, list[str], str] | None:
    if isinstance(previous, DEFINITIONS) or isinstance(current, DEFINITIONS):
        return None

    if previous.end_lineno >= current.lineno:
        return None

    if any(line in disabled for line in range(previous.lineno, current.end_lineno + 1)):
        return None

    start, end = previous.end_lineno, current.lineno - 1

    # A blank line can terminate an explicit backslash continuation
    if lines[start - 1].rstrip("\r\n").endswith("\\"):
        return None
    start = _separator_start(previous, current, lines)
    gap = lines[start:end]
    attached = any(line.strip() for line in gap)

    if isinstance(current, CONTROL_FLOW):
        header, body, bound = _flow(current)
        preceding = (
            set()
            if isinstance(previous, CONTROL_BLOCKS)
            else _names(previous, previous.end_lineno, bound=set(), stores=True)
        )

        if not attached and body is not None and preceding & _names(body, body.lineno, bound=bound):
            if gap:
                return start, end, [], "Unnecessary blank line before related control flow"
            return None

        if any(preceding & _names(expr, current.lineno, bound=set()) for expr in header) and not (
            _construction(previous) or _construction(current)
        ):
            return None
        message = f"Missing blank line above `{type(current).__name__.lower()}`"
    elif _construction(previous) or _construction(current):
        message = "Missing blank line around multiline construction"
    else:
        return None

    # Put the separator above attached comments without moving or rewriting them
    if attached:
        end = next(index for index in range(start, end) if lines[index].strip())

    if end > start:
        return None
    newline = "\r\n" if lines[start - 1].endswith("\r\n") else "\n"
    return start, end, [newline], message


def _separator_start(previous: ast.stmt, current: ast.stmt, lines: list[str]) -> int:
    start = previous.end_lineno
    if not isinstance(previous, CONTROL_BLOCKS):
        return start

    # Deeper comments belong to the preceding suite
    for index in range(start, current.lineno - 1):
        line = lines[index]
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" \t"))
        if indent <= current.col_offset:
            break
        start = index + 1
    return start


def _flow(node: ast.stmt) -> tuple[list[ast.expr], ast.stmt | None, set[str]]:
    guard = None

    match node:
        case ast.If(test=expression, body=body) | ast.While(test=expression, body=body):
            first = body[0]
            bound = set()
        case (
            ast.For(iter=expression, target=target, body=body)
            | ast.AsyncFor(
                iter=expression,
                target=target,
                body=body,
            )
        ):
            bound = {name.id for name in ast.walk(target) if isinstance(name, ast.Name)}
            first = body[0]
        case ast.Match(subject=expression, cases=cases):
            bound = {
                name
                for node in ast.walk(cases[0].pattern)
                for field, name in ast.iter_fields(node)
                if field in {"name", "rest"} and isinstance(name, str)
            }
            first = cases[0].body[0]
            guard = cases[0].guard
        case _:
            return [], None, set()
    scan = _Names(node.lineno, set(), stores=False)
    scan.visit(expression)
    if guard is not None:
        scan.visit(guard)
    return [expression], first, bound | scan.bound


def _construction(node: ast.stmt) -> bool:
    match node:
        case ast.Assign(value=value) | ast.AnnAssign(value=value) | ast.Expr(value=value):
            pass
        case ast.Return(value=value):
            pass
        case _:
            return False

    while isinstance(value, (ast.Await, ast.Yield, ast.YieldFrom)):
        value = value.value

    if not isinstance(value, (ast.List, ast.Tuple, ast.Set, ast.Dict, ast.Call)):
        return False

    if value.lineno == value.end_lineno:
        return False

    if isinstance(value, ast.Call):
        match value.func:
            case ast.Name(id=name) | ast.Attribute(attr=name):
                return name.lstrip("_")[:1].isupper()
            case _:
                return False
    return True


def _names(node: ast.AST, line: int, *, bound: set[str], stores: bool = False) -> set[str]:
    scan = _Names(line, bound, stores=stores)
    scan.visit(node)
    return scan.found


class _Names(ast.NodeVisitor):
    def __init__(self, line: int, bound: set[str], *, stores: bool) -> None:
        self.line = line
        self.bound = bound.copy()
        self.stores = stores
        self.found: set[str] = set()

    def visit_Name(self, node: ast.Name) -> None:
        if (
            node.lineno == self.line
            and node.id not in self.bound
            and node.id not in {"self", "cls"}
            and not node.id.lstrip("_")[:1].isupper()
            and (self.stores or isinstance(node.ctx, ast.Load))
        ):
            self.found.add(node.id)

    def visit_Attribute(self, node: ast.Attribute) -> None:
        if isinstance(node.value, ast.Name) and node.value.id in {"self", "cls"}:
            if (
                node.lineno == self.line
                and node.value.id not in self.bound
                and (self.stores or isinstance(node.ctx, ast.Load))
            ):
                self.found.add(f"{node.value.id}.{node.attr}")
        else:
            self.visit(node.value)

    def visit_AugAssign(self, node: ast.AugAssign) -> None:
        target = copy.copy(node.target)
        target.ctx = ast.Load()
        self.visit(target)
        self.visit(node.value)

    def visit_AnnAssign(self, node: ast.AnnAssign) -> None:
        self.visit(node.target)
        if node.value is not None:
            self.visit(node.value)

    def visit_Call(self, node: ast.Call) -> None:
        if isinstance(node.func, ast.Attribute):
            self.visit(node.func.value)
        elif not isinstance(node.func, ast.Name):
            self.visit(node.func)

        for argument in [*node.args, *node.keywords]:
            self.visit(argument)

    def visit_NamedExpr(self, node: ast.NamedExpr) -> None:
        self.visit(node.value)
        if self.stores:
            self.visit(node.target)
        self.bound.add(node.target.id)

    def visit_Lambda(self, node: ast.Lambda) -> None:
        for default in [*node.args.defaults, *node.args.kw_defaults]:
            if default is not None:
                self.visit(default)
        previous = self.bound.copy()
        self.bound.update(arg.arg for arg in ast.walk(node.args) if isinstance(arg, ast.arg))
        self.visit(node.body)
        self.bound = previous

    def visit_ListComp(self, node: ast.ListComp) -> None:
        self._comprehension(node.generators, [node.elt])

    def visit_SetComp(self, node: ast.SetComp) -> None:
        self._comprehension(node.generators, [node.elt])

    def visit_DictComp(self, node: ast.DictComp) -> None:
        self._comprehension(node.generators, [node.key, node.value])

    def visit_GeneratorExp(self, node: ast.GeneratorExp) -> None:
        self._comprehension(node.generators, [node.elt])

    def _comprehension(self, generators: list[ast.comprehension], values: list[ast.expr]) -> None:
        previous = self.bound.copy()
        targets = set()

        for generator in generators:
            self.visit(generator.iter)
            bound = {node.id for node in ast.walk(generator.target) if isinstance(node, ast.Name)}
            targets.update(bound)
            self.bound.update(bound)

            for condition in generator.ifs:
                self.visit(condition)

        for value in values:
            self.visit(value)
        self.bound = (self.bound - targets) | previous

    def visit_FunctionDef(self, node: ast.FunctionDef) -> None:
        pass

    def visit_AsyncFunctionDef(self, node: ast.AsyncFunctionDef) -> None:
        pass

    def visit_ClassDef(self, node: ast.ClassDef) -> None:
        pass


if __name__ == "__main__":
    sys.exit(_main())
