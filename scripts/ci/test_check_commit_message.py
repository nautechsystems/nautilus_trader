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
Test the commit message policy checker.

Exercise the checker through its public API and its command line interface, including a
synthetic git repository for the --ci-range mode.

"""

import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from check_commit_message import SUBJECT_MAX_GUIDANCE
from check_commit_message import check_message


CHECKER = Path(__file__).with_name("check_commit_message.py").resolve()


def test_valid_message() -> None:
    """
    Test that valid messages pass without errors or warnings.
    """
    message = (
        "Refine commit message policy\n\n"
        "Developed with assistance from AI.\n\n"
        "Co-authored-by: Claude Shannon <claude@example.com>\n"
        "Co-authored-by: Devin Smith <devin@example.com>"
    )

    assert check_message(message) == ([], [])
    assert check_message("Fix policy") == ([], [])


def test_subject_rules() -> None:
    """
    Test subject length, capitalization, punctuation, and syntax rules.
    """
    cases = {
        "Fix typo": "subject must contain at least 10 characters",
        "fix: Reject invalid order state": ("subject must not use Conventional Commits syntax"),
        "refine invalid order state": "subject must start with a capitalized word",
        "Fix invalid order state.": "subject must not end with a period",
    }

    for subject, expected in cases.items():
        errors, _ = check_message(subject)
        assert expected in errors


def test_subject_number_references() -> None:
    """
    Test that issue and pull request numbers are rejected in subjects.
    """
    expected = "subject must not include an issue or pull request number"
    subjects = (
        "Fix invalid order state (#123)",
        "Fix invalid order state #123",
        "Fix invalid order state [#123]",
        "Fix PR #123 review feedback",
        "Fixes #123 for invalid order state",
    )

    for subject in subjects:
        errors, _ = check_message(subject)
        assert expected in errors

    assert check_message("Fix invalid order state\n\nResolves #123") == ([], [])


def test_long_subject_warns_without_failing() -> None:
    """
    Test that a subject beyond the length guidance warns without failing.
    """
    target_subject = f"Refine {'x' * 53}"
    subject = f"Refine {'x' * 54}"
    errors, warnings = check_message(subject)

    assert len(target_subject) == SUBJECT_MAX_GUIDANCE
    assert check_message(target_subject) == ([], [])
    assert len(subject) == SUBJECT_MAX_GUIDANCE + 1
    assert errors == []
    assert warnings == [
        (
            f"subject contains {len(subject)} characters; aim for 60 or fewer because this limit "
            "will be enforced in the future"
        ),
    ]


def test_body_rules() -> None:
    """
    Test body separator and line-length rules.
    """
    long_line = "x" * 80

    assert check_message("Refine commit policy\nBody without a separator")[0] == [
        "body must be separated from the subject by a blank line",
    ]
    assert check_message(f"Refine commit policy\n\n{long_line}")[0] == [
        "line 3 contains 80 characters; body lines must contain 79 or fewer",
    ]
    assert check_message(f"Refine commit policy\n\n{'x' * 79}") == ([], [])


def test_branded_attribution() -> None:
    """
    Test that branded AI attribution lines are rejected.
    """
    message = (
        "Generated with GitHub Copilot\n\n"
        "Powered by OpenAI\n"
        "Made with Claude\n"
        "AI-assisted by Copilot\n"
        "With assistance from Claude\n"
        "Thanks to OpenAI"
    )
    errors, _ = check_message(message)

    assert errors == [
        "line 1 contains branded AI attribution",
        "line 3 contains branded AI attribution",
        "line 4 contains branded AI attribution",
        "line 5 contains branded AI attribution",
        "line 6 contains branded AI attribution",
        "line 7 contains branded AI attribution",
    ]


def test_wrapped_branded_attribution() -> None:
    """
    Test branded attribution wrapped across consecutive lines.
    """
    errors, _ = check_message("Refine policy\n\nGenerated\nwith Claude Code")

    assert errors == ["line 3 contains branded AI attribution"]
    assert check_message("Refine policy\n\nGenerated with\n\nClaude Code") == ([], [])


def test_ai_coauthor_trailers() -> None:
    """
    Test that AI co-author trailers are rejected.
    """
    message = (
        "Refine commit message policy\n\n"
        "Co-authored-by: Copilot <copilot@example.com>\n"
        "Co-authored-by: AI Assistant <assistant@example.com>\n"
        "Co-authored-by: Tool <noreply@anthropic.com>\n"
        "Co-authored-by: AI <ai@example.com>\n"
        "Co-authored-by: Claude Sonnet <model@example.com>\n"
        "Co-authored-by: Claude 3.5 Sonnet <model@example.com>\n"
        "Co-authored-by: Claude 3 Opus <model@example.com>"
    )
    errors, _ = check_message(message)

    assert errors == [
        "line 3 contains an AI co-author trailer",
        "line 4 contains an AI co-author trailer",
        "line 5 contains an AI co-author trailer",
        "line 6 contains an AI co-author trailer",
        "line 7 contains an AI co-author trailer",
        "line 8 contains an AI co-author trailer",
        "line 9 contains an AI co-author trailer",
    ]


def test_message_file_cli() -> None:
    """
    Test the message file command line interface end to end.
    """
    result = run_message_file("Fix typo")

    assert result.returncode == 1
    assert result.stdout == ("ERROR: commit message: subject must contain at least 10 characters\n")

    subject = f"Refine {'x' * 54}"
    result = run_message_file(subject)

    assert result.returncode == 0
    assert result.stdout == (
        f"WARNING: commit message: subject contains {len(subject)} characters; "
        "aim for 60 or fewer because this limit will be enforced in the future\n"
    )

    message = (
        "Refine commit policy\n\n"
        "Keep the authored body.\n"
        "# Generated with Claude Code\n"
        "# ------------------------ >8 ------------------------\n"
        "Generated with Claude Code\n"
        "Co-authored-by: Codex <codex@example.com>\n"
    )
    result = run_message_file(message)

    assert result.returncode == 0
    assert result.stdout == ""


def test_ci_range() -> None:
    """
    Test the --ci-range mode against a synthetic git repository.
    """
    with tempfile.TemporaryDirectory() as directory:
        repo = Path(directory)
        git(repo, "init", "--quiet")
        hooks = repo / "hooks"
        hooks.mkdir()
        git(repo, "config", "core.hooksPath", str(hooks))
        git(repo, "config", "user.name", "Human Contributor")
        git(repo, "config", "user.email", "contributor@example.com")
        git(repo, "config", "commit.gpgsign", "false")
        git(repo, "commit", "--allow-empty", "--message", "Refine base policy")
        base = git(repo, "rev-parse", "HEAD")
        git(repo, "commit", "--allow-empty", "--message", "Refine invalid policy.")
        invalid = git(repo, "rev-parse", "HEAD")
        git(repo, "commit", "--allow-empty", "--message", "fix: Synthetic merge candidate")

        event_path = repo / "event.json"
        event_path.write_text(
            json.dumps({"pull_request": {"head": {"sha": invalid}}}),
            encoding="utf-8",
        )

        env = os.environ.copy()
        env.pop("GITHUB_ACTIONS", None)
        env["CHANGED_BASE_SHA"] = base
        env["GITHUB_EVENT_NAME"] = "pull_request"
        env["GITHUB_EVENT_PATH"] = str(event_path)
        result = subprocess.run(
            [sys.executable, "-B", str(CHECKER), "--ci-range"],
            capture_output=True,
            check=False,
            cwd=repo,
            env=env,
            text=True,
        )

        assert result.returncode == 1
        assert result.stdout == (
            f"ERROR: commit {invalid[:12]}: subject must not end with a period\n"
        )

        env["GITHUB_EVENT_NAME"] = "push"
        result = subprocess.run(
            [sys.executable, "-B", str(CHECKER), "--ci-range"],
            capture_output=True,
            check=False,
            cwd=repo,
            env=env,
            text=True,
        )

        assert result.returncode == 0
        assert result.stdout == ""


def run_message_file(message: str) -> subprocess.CompletedProcess[str]:
    """
    Run the checker on a message file and return the completed process.
    """
    with tempfile.TemporaryDirectory() as directory:
        path = Path(directory, "COMMIT_EDITMSG")
        path.write_text(message, encoding="utf-8")
        env = os.environ.copy()
        env.pop("GITHUB_ACTIONS", None)

        return subprocess.run(
            [sys.executable, "-B", str(CHECKER), str(path)],
            capture_output=True,
            check=False,
            env=env,
            text=True,
        )


def git(repo: Path, *args: str) -> str:
    """
    Run a git command in the repository and return its stripped stdout.
    """
    executable = shutil.which("git")
    if executable is None:
        raise RuntimeError("git executable is not available")

    result = subprocess.run(
        [executable, *args],
        capture_output=True,
        check=True,
        cwd=repo,
        text=True,
    )

    return result.stdout.strip()


def main() -> None:
    """
    Run all commit message policy check tests.
    """
    test_valid_message()
    test_subject_rules()
    test_subject_number_references()
    test_long_subject_warns_without_failing()
    test_body_rules()
    test_branded_attribution()
    test_wrapped_branded_attribution()
    test_ai_coauthor_trailers()
    test_message_file_cli()
    test_ci_range()
    print("Commit message policy check tests passed")


if __name__ == "__main__":
    main()
