#!/usr/bin/env python3
# Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
# Licensed under the GNU Lesser General Public License Version 3.0.
# See https://www.gnu.org/licenses/lgpl-3.0.en.html for license details.
"""
Exercise allowance generation and lint precedence with isolated Cargo/Clippy probes.
"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path


REPO_ROOT = Path(__file__).resolve().parents[2]
GENERATOR = REPO_ROOT / "scripts/update-clippy-allowances.py"
GIT = shutil.which("git")
CARGO = shutil.which("cargo")
CARGO_FAILURE = 101
OPT_IN = "#![warn(clippy::pedantic)]\n"
SIMILAR_NAMES = (
    "#[must_use]\npub fn combine(black_hole: u8, black_holy: u8) -> u8 {\n"
    "    black_hole.wrapping_add(black_holy)\n}\n"
)
CAST = "#[must_use]\npub fn narrow(value: u64) -> u8 { value as u8 }\n"


def _workspace(root: Path, lints: str) -> None:
    (root / "src").mkdir()
    (root / "scripts").mkdir()
    shutil.copyfile(GENERATOR, root / "scripts" / GENERATOR.name)
    (root / ".gitignore").write_text("target/\n", encoding="utf-8")
    (root / "Cargo.toml").write_text(
        '[package]\nname="lint-probe"\nversion="0.0.0"\nedition="2024"\n'
        "[lints]\nworkspace=true\n[workspace]\n[workspace.lints.clippy]\n" + lints,
        encoding="utf-8",
    )
    assert GIT is not None
    subprocess.run([GIT, "init", "--quiet"], cwd=root, check=True)


def _generate(root: Path, *arguments: str) -> subprocess.CompletedProcess[str]:
    command = [sys.executable, "-B", str(root / "scripts" / GENERATOR.name), *arguments]
    return subprocess.run(command, cwd=root, capture_output=True, text=True, check=False)


def _test_generation(root: Path) -> None:
    _workspace(
        root,
        'similar_names="allow"\nmanual_let_else={level="allow",priority=100}\ndbg_macro="warn"\n',
    )
    paths = [root / "src" / name for name in ("lib.rs", "main.rs", "nested.rs")]
    source = OPT_IN + SIMILAR_NAMES
    for path in paths:
        path.write_text(source, encoding="utf-8")
    (root / "src/unmigrated.rs").write_text(CAST, encoding="utf-8")

    flags = _generate(root, "--flags")
    assert flags.returncode == 0
    assert flags.stdout == "-Aclippy::manual_let_else -Aclippy::similar_names\n"
    stale = _generate(root, "--check")
    assert stale.returncode == 1
    assert set(stale.stdout.splitlines()) == {f"src/{path.name}" for path in paths}
    assert "Run python3 -B scripts/update-clippy-allowances.py" in stale.stderr
    assert all(path.read_text(encoding="utf-8") == source for path in paths)

    assert _generate(root).returncode == 0
    generated = paths[0].read_text(encoding="utf-8")
    assert "clippy::manual_let_else,\n    clippy::similar_names," in generated
    assert "clippy::dbg_macro" not in generated
    assert all(path.read_text(encoding="utf-8") == generated for path in paths)
    assert (root / "src/unmigrated.rs").read_text(encoding="utf-8") == CAST
    assert _generate(root, "--check").returncode == 0
    assert _generate(root).stdout == ""

    paths[0].write_text(
        source + generated.removeprefix(OPT_IN).removesuffix(SIMILAR_NAMES),
        encoding="utf-8",
    )
    assert _generate(root, "--check").returncode == 1
    assert _generate(root).returncode == 0
    assert paths[0].read_text(encoding="utf-8") == generated

    manifest = root / "Cargo.toml"
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'similar_names="allow"',
            'similar_names="warn"',
        ),
        encoding="utf-8",
    )
    assert _generate(root, "--check").returncode == 1
    assert _generate(root).returncode == 0
    assert "clippy::similar_names" not in paths[0].read_text(encoding="utf-8")
    paths[0].write_text(generated.removeprefix(OPT_IN), encoding="utf-8")
    assert _generate(root).returncode == 0
    assert paths[0].read_text(encoding="utf-8") == SIMILAR_NAMES


def _clippy(root: Path, flags: tuple[str, ...] = ()) -> tuple[int, set[tuple[str, str]]]:
    assert CARGO is not None
    with (REPO_ROOT / "rust-toolchain.toml").open("rb") as toolchain:
        channel = tomllib.load(toolchain)["toolchain"]["channel"]
    environment = dict(
        os.environ,
        CARGO_BUILD_JOBS="2",
        CARGO_TARGET_DIR=str(root / "target"),
        RUSTC_WRAPPER="",
        RUSTC_WORKSPACE_WRAPPER="",
        RUSTFLAGS="",
    )
    environment.pop("CARGO_ENCODED_RUSTFLAGS", None)
    result = subprocess.run(
        [
            CARGO,
            f"+{channel}",
            "clippy",
            "--offline",
            "--quiet",
            "--all-targets",
            "--message-format=json",
            "--",
            "-D",
            "warnings",
            *flags,
        ],
        cwd=root,
        env=environment,
        capture_output=True,
        text=True,
        check=False,
    )
    diagnostics = set()
    for line in result.stdout.splitlines():
        event = json.loads(line)
        if event.get("reason") == "compiler-message":
            message = event["message"]
            if message.get("code"):
                diagnostics.add((message["code"]["code"], message["level"]))
    assert result.returncode in (0, CARGO_FAILURE), result.stderr
    return result.returncode, diagnostics


def _test_opt_in_forms(root: Path) -> None:
    _workspace(root, 'similar_names="allow"\n')
    library = root / "src/lib.rs"
    source = (
        '#![deny(clippy::pedantic, reason = "crate opt-in")]\n'
        "pub mod nested {\n"
        "    #![warn(\n        clippy::pedantic,\n"
        "        clippy::cast_possible_truncation\n    )]\n" + SIMILAR_NAMES + "}\n"
    )
    library.write_text(source, encoding="utf-8")
    assert _generate(root).returncode == 0
    generated = library.read_text(encoding="utf-8")
    assert generated.count("clippy::similar_names,") == 2  # noqa: PLR2004
    assert "    #![allow(\n        clippy::similar_names," in generated
    assert _generate(root, "--check").returncode == 0
    assert _clippy(root) == (0, set())
    manifest = root / "Cargo.toml"
    manifest.write_text(
        manifest.read_text(encoding="utf-8").replace(
            'similar_names="allow"',
            'similar_names="warn"',
        ),
        encoding="utf-8",
    )
    assert _generate(root).returncode == 0
    assert library.read_text(encoding="utf-8") == source


def _test_precedence(root: Path) -> None:
    manifest = (REPO_ROOT / "Cargo.toml").read_text(encoding="utf-8")
    lints = manifest.split("[workspace.lints.clippy]\n", 1)[1].split("[workspace.lints.rust]", 1)[0]
    _workspace(root, lints)
    library = root / "src/lib.rs"
    library.write_text(OPT_IN, encoding="utf-8")
    assert _generate(root).returncode == 0
    enabled = library.read_text(encoding="utf-8")
    allowances = enabled.removeprefix(OPT_IN)
    command = _generate(root, "--flags")
    assert command.returncode == 0
    flags_opt_in = ("-Wclippy::pedantic", *command.stdout.split())
    local_exception = (
        '#[allow(clippy::cast_possible_truncation, reason = "probe intentionally truncates")]\n'
    )
    blockchain = (REPO_ROOT / "crates/adapters/blockchain/src/lib.rs").read_text(encoding="utf-8")
    blockchain_attributes = blockchain[blockchain.index("#![warn(rustc::all)]") :].split(
        "\npub mod ",
        1,
    )[0]
    cases = [
        (
            "blockchain_workspace_check",
            blockchain_attributes
            + "\n#[must_use]\npub fn mapped(value: Option<u8>) -> u8 { "
            + "value.map(|value| value.wrapping_add(1)).unwrap_or(0) }\n",
            "",
            (),
            CARGO_FAILURE,
            "map_unwrap_or",
        ),
        ("blockchain_staged_adoption", blockchain_attributes + CAST, "", (), 0, ""),
        ("cargo_source_precedence", OPT_IN + SIMILAR_NAMES, "", (), CARGO_FAILURE, "similar_names"),
        ("source_allowance", enabled + SIMILAR_NAMES, "", (), 0, ""),
        ("deny_opt_in", enabled.replace("warn(", "deny(") + SIMILAR_NAMES, "", (), 0, ""),
        ("command_opt_in", SIMILAR_NAMES, "", flags_opt_in, 0, ""),
        ("command_check", CAST, "", flags_opt_in, CARGO_FAILURE, "cast_possible_truncation"),
        ("command_exception", local_exception + CAST, "", flags_opt_in, 0, ""),
        (
            "source_order",
            allowances + OPT_IN + SIMILAR_NAMES,
            "",
            (),
            CARGO_FAILURE,
            "similar_names",
        ),
        (
            "inner_inheritance",
            enabled + "pub mod nested {\n" + SIMILAR_NAMES + "}\n",
            "",
            (),
            0,
            "",
        ),
        (
            "inner_opt_in",
            enabled + "pub mod nested {\n" + OPT_IN + SIMILAR_NAMES + "}\n",
            "",
            (),
            CARGO_FAILURE,
            "similar_names",
        ),
        (
            "inner_allowance",
            enabled + "pub mod nested {\n" + enabled + SIMILAR_NAMES + "}\n",
            "",
            (),
            0,
            "",
        ),
        (
            "pedantic_still_enabled",
            enabled + CAST,
            "",
            (),
            CARGO_FAILURE,
            "cast_possible_truncation",
        ),
        ("local_exception", enabled + local_exception + CAST, "", (), 0, ""),
        (
            "local_opt_in",
            enabled + "#[warn(clippy::similar_names)]\n" + SIMILAR_NAMES,
            "",
            (),
            CARGO_FAILURE,
            "similar_names",
        ),
        (
            "separate_roots",
            enabled + SIMILAR_NAMES,
            enabled + SIMILAR_NAMES + "fn main() {}\n",
            (),
            0,
            "",
        ),
        ("unmigrated_binary", enabled + SIMILAR_NAMES, CAST + "fn main() {}\n", (), 0, ""),
        (
            "binary_opt_in",
            SIMILAR_NAMES,
            enabled + CAST + "fn main() {}\n",
            (),
            CARGO_FAILURE,
            "cast_possible_truncation",
        ),
        (
            "workspace_check",
            enabled + "#[must_use]\npub fn marker(value: u8) -> u8 { dbg!(value) }\n",
            "",
            (),
            CARGO_FAILURE,
            "dbg_macro",
        ),
        (
            "forced_audit",
            enabled + SIMILAR_NAMES,
            "",
            ("--force-warn", "clippy::similar_names"),
            0,
            "similar_names",
        ),
    ]
    for name, source, binary, flags, status, lint in cases:
        library.write_text(source, encoding="utf-8")
        binary_path = root / "src/main.rs"
        if binary:
            binary_path.write_text(binary, encoding="utf-8")
        else:
            binary_path.unlink(missing_ok=True)
        result, diagnostics = _clippy(root, flags)
        expected = {(f"clippy::{lint}", "error" if status else "warning")} if lint else set()
        assert (result, diagnostics) == (status, expected), (name, result, diagnostics)

    bodies = {
        "similar_names": SIMILAR_NAMES,
        "manual_let_else": (
            "#[must_use]\npub fn choose(option: Option<u8>) -> u8 {\n"
            "let value = match option { Some(value) => value, None => return 0 };\n"
            "value.wrapping_add(1)\n}\n"
        ),
        "single_match_else": (
            "#[must_use]\npub fn choose(option: Option<u8>) -> u8 {\n"
            "match option { Some(value) => { std::hint::black_box(value); value },\n"
            "None => { std::hint::black_box(4); 5 } }\n}\n"
        ),
        "redundant_else": (
            "#[must_use]\npub fn choose(flag: bool) -> u8 {\n"
            "if flag { return 1; } else { std::hint::black_box(2); } 3\n}\n"
        ),
        "or_fun_call": (
            "#[must_use]\npub fn choose(option: Option<String>) -> String {\n"
            'option.unwrap_or(String::from("fallback"))\n}\n'
        ),
        "option_if_let_else": (
            "#[must_use]\npub fn choose(option: Option<u8>) -> u8 {\n"
            "if let Some(value) = option { value.wrapping_add(1) } else { 0 }\n}\n"
        ),
    }
    for lint, body in bodies.items():
        opt_in = f"#![warn(clippy::{lint})]\n"
        library.write_text(opt_in + OPT_IN + body, encoding="utf-8")
        result = _clippy(root)
        assert result == (CARGO_FAILURE, {(f"clippy::{lint}", "error")}), (lint, result)
        library.write_text(opt_in + enabled + body, encoding="utf-8")
        assert _clippy(root) == (0, set()), lint

    library.write_text(OPT_IN + SIMILAR_NAMES, encoding="utf-8")
    path = root / "Cargo.toml"
    path.write_text(
        path.read_text(encoding="utf-8").replace(
            'similar_names = "allow"',
            'similar_names = { level = "allow", priority = 100 }',
        ),
        encoding="utf-8",
    )
    assert _clippy(root) == (CARGO_FAILURE, {("clippy::similar_names", "error")})
    path.write_text(
        path.read_text(encoding="utf-8") + '\n[lints.clippy]\npedantic="warn"\n',
        encoding="utf-8",
    )
    result = subprocess.run(
        [CARGO, "metadata", "--offline", "--no-deps"],
        cwd=root,
        capture_output=True,
        text=True,
        check=False,
    )
    assert result.returncode == CARGO_FAILURE
    assert "cannot override" in result.stderr


if __name__ == "__main__":
    with tempfile.TemporaryDirectory(prefix="nautilus-clippy-generation-") as directory:
        _test_generation(Path(directory))
    with tempfile.TemporaryDirectory(prefix="nautilus-clippy-precedence-") as directory:
        _test_precedence(Path(directory))
    with tempfile.TemporaryDirectory(prefix="nautilus-clippy-opt-ins-") as directory:
        _test_opt_in_forms(Path(directory))
    print("Clippy allowance generation and compiler precedence probes passed")
