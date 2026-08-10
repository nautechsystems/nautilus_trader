#!/usr/bin/env python3

import argparse
import json
import os
import re
import shutil
import subprocess
import sys
from pathlib import Path


SUBJECT_MIN_LENGTH = 10
SUBJECT_MAX_GUIDANCE = 60
BODY_MAX_LENGTH = 79

SCISSORS_LINE = re.compile(
    r"^\s*[^\w\s]\s+------------------------ >8 ------------------------\s*$",
)

CONVENTIONAL_SUBJECT = re.compile(
    r"^(?:build|chore|ci|docs|feat|fix|perf|refactor|revert|style|test)"
    r"(?:\([^()\r\n]+\))?!?:",
    re.IGNORECASE,
)
MANUAL_PR_SUFFIX = re.compile(r"\s+\(#\d+\)$")
COAUTHOR_TRAILER = re.compile(r"^\s*co-authored-by\s*:", re.IGNORECASE)
ATTRIBUTION_PHRASE = re.compile(
    r"\b(?:"
    r"(?:authored|built|created|developed|generated|made|powered|produced|written)"
    r"\s+(?:by|using|with)|(?:ai\W+)?assisted\s+(?:by|using|with)|"
    r"thanks\s+to|with\s+(?:ai\s+)?assistance\s+from"
    r")\b",
    re.IGNORECASE,
)
AI_BRAND = re.compile(
    r"\b(?:"
    r"anthropic|chatgpt|claude(?:\s+code)?|codex|copilot|cursor|devin|gemini|"
    r"github\s+copilot|google\s+ai|jules|openai|windsurf"
    r")\b",
    re.IGNORECASE,
)
AI_IDENTITY_MARKER = re.compile(
    r"(?:^|[^a-z0-9])(?:"
    r"ai[^a-z0-9]+(?:agent|assistant|bot|model|tool)|anthropic|chatgpt|"
    r"claude[^a-z0-9]+(?:"
    r"agent|bot|code|haiku|opus|sonnet|"
    r"v?\d+(?:\.\d+)*(?:[^a-z0-9]+(?:haiku|opus|sonnet))?"
    r")|codex|copilot|"
    r"cursor[^a-z0-9]+(?:agent|bot|ai)|devin[^a-z0-9]+(?:agent|bot|ai)|"
    r"gemini[^a-z0-9]+(?:agent|bot|code)|google[^a-z0-9]+ai|"
    r"jules[^a-z0-9]+(?:agent|bot)|openai|"
    r"windsurf[^a-z0-9]+(?:agent|bot|ai)"
    r")(?:$|[^a-z0-9])",
    re.IGNORECASE,
)
AI_IDENTITY_NAMES = frozenset(
    {
        "ai",
        "anthropic",
        "chatgpt",
        "claude",
        "codex",
        "copilot",
        "cursor",
        "devin",
        "gemini",
        "jules",
        "openai",
        "windsurf",
    },
)


def check_message(message: str) -> tuple[list[str], list[str]]:
    lines = message.splitlines()
    subject = lines[0] if lines else ""
    errors = _check_subject(subject)
    errors.extend(_check_body(lines))
    errors.extend(_check_attribution(message))

    warnings = []
    if len(subject) > SUBJECT_MAX_GUIDANCE:
        warnings.append(
            f"subject contains {len(subject)} characters; aim for {SUBJECT_MAX_GUIDANCE} or fewer "
            "because this limit will be enforced in the future",
        )

    return errors, warnings


def _check_subject(subject: str) -> list[str]:
    errors = []

    if len(subject) < SUBJECT_MIN_LENGTH:
        errors.append(f"subject must contain at least {SUBJECT_MIN_LENGTH} characters")

    if not subject or not subject[0].isupper():
        errors.append("subject must start with a capitalized word")

    if subject.endswith("."):
        errors.append("subject must not end with a period")

    if CONVENTIONAL_SUBJECT.match(subject):
        errors.append("subject must not use Conventional Commits syntax")

    if MANUAL_PR_SUFFIX.search(subject):
        errors.append("subject must not include a pull request number suffix")

    return errors


def _check_body(lines: list[str]) -> list[str]:
    if len(lines) < 2 or not any(line.strip() for line in lines[1:]):
        return []

    errors = []
    if lines[1].strip():
        errors.append("body must be separated from the subject by a blank line")

    for line_number, line in enumerate(lines[1:], start=2):
        if len(line) > BODY_MAX_LENGTH:
            errors.append(
                f"line {line_number} contains {len(line)} characters; "
                f"body lines must contain {BODY_MAX_LENGTH} or fewer",
            )

    return errors


def _check_attribution(message: str) -> list[str]:
    errors = []
    lines = message.splitlines()
    branded_lines = {
        line_number
        for line_number, line in enumerate(lines, start=1)
        if ATTRIBUTION_PHRASE.search(line) and AI_BRAND.search(line)
    }

    paragraph_start = 0
    paragraph_lines = []
    for line_number, line in enumerate((*lines, ""), start=1):
        if line.strip():
            if not paragraph_lines:
                paragraph_start = line_number
            paragraph_lines.append(line)
            continue

        if paragraph_lines:
            paragraph_range = range(paragraph_start, line_number)
            paragraph = " ".join(paragraph_lines)
            if (
                not branded_lines.intersection(paragraph_range)
                and ATTRIBUTION_PHRASE.search(paragraph)
                and AI_BRAND.search(paragraph)
            ):
                branded_lines.add(paragraph_start)
            paragraph_lines = []

    for line_number, line in enumerate(lines, start=1):
        if COAUTHOR_TRAILER.match(line) and _is_ai_identity(line.partition(":")[2]):
            errors.append(f"line {line_number} contains an AI co-author trailer")
        elif line_number in branded_lines:
            errors.append(f"line {line_number} contains branded AI attribution")

    return errors


def _is_ai_identity(identity: str) -> bool:
    name = identity.partition("<")[0].strip()

    return name.casefold() in AI_IDENTITY_NAMES or bool(AI_IDENTITY_MARKER.search(identity))


def _git_output(*args: str, input_text: str | None = None) -> str:
    git = shutil.which("git")
    if git is None:
        raise ValueError("git executable is not available")

    result = subprocess.run(
        [git, *args],
        capture_output=True,
        check=False,
        input=input_text,
        text=True,
    )
    if result.returncode != 0:
        detail = result.stderr.strip() or result.stdout.strip() or "unknown error"
        raise ValueError(f"git {' '.join(args)} failed: {detail}")

    return result.stdout


def _clean_message_file(message: str) -> str:
    lines = []
    for line in message.splitlines(keepends=True):
        if SCISSORS_LINE.match(line.rstrip("\r\n")):
            break
        lines.append(line)

    return _git_output("stripspace", "--strip-comments", input_text="".join(lines))


def _messages_from_range(base: str, head: str) -> list[tuple[str, str]]:
    if not re.fullmatch(r"[0-9a-fA-F]{40}", base):
        raise ValueError("CHANGED_BASE_SHA must contain a 40-character Git SHA")
    if not re.fullmatch(r"[0-9a-fA-F]{40}", head):
        raise ValueError("pull request head must contain a 40-character Git SHA")

    merge_base = _git_output("merge-base", base, head).strip()
    if not re.fullmatch(r"[0-9a-fA-F]{40}", merge_base):
        raise ValueError("failed to resolve the commit message comparison base")

    shas = _git_output("rev-list", "--reverse", f"{merge_base}..{head}").splitlines()

    return [(sha, _git_output("show", "-s", "--format=%B", sha)) for sha in shas]


def _pull_request_head() -> str:
    event_path = os.environ.get("GITHUB_EVENT_PATH")
    if not event_path:
        raise ValueError("GITHUB_EVENT_PATH is required for pull request commit validation")

    try:
        event = json.loads(Path(event_path).read_text(encoding="utf-8"))
        head = event["pull_request"]["head"]["sha"]
    except (KeyError, OSError, TypeError, UnicodeError, json.JSONDecodeError) as e:
        raise ValueError(f"failed to read the pull request head SHA: {e}") from e

    if not isinstance(head, str):
        raise ValueError("pull request head SHA is not text")

    return head


def _print_error(message: str) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::error title=Commit message policy::{message}")
    else:
        print(f"ERROR: {message}")


def _print_warning(message: str) -> None:
    if os.environ.get("GITHUB_ACTIONS") == "true":
        print(f"::warning title=Commit message guidance::{message}")
    else:
        print(f"WARNING: {message}")


def _report_message(label: str, message: str) -> bool:
    errors, warnings = check_message(message)

    for error in errors:
        _print_error(f"{label}: {error}")
    for warning in warnings:
        _print_warning(f"{label}: {warning}")

    return bool(errors)


def _check_ci_range() -> int:
    if os.environ.get("GITHUB_EVENT_NAME") != "pull_request":
        return 0

    base = os.environ.get("CHANGED_BASE_SHA")
    if not base:
        _print_error("CHANGED_BASE_SHA is required for pull request commit validation")
        return 1

    try:
        messages = _messages_from_range(base, _pull_request_head())
    except ValueError as e:
        _print_error(str(e))
        return 1

    failed = False
    for sha, message in messages:
        failed |= _report_message(f"commit {sha[:12]}", message)

    return int(failed)


def main() -> int:
    parser = argparse.ArgumentParser(description="Check commit messages against project policy.")
    parser.add_argument("message_file", nargs="?", type=Path, help="Git commit message file")
    parser.add_argument(
        "--ci-range",
        action="store_true",
        help="check pull request commits from CHANGED_BASE_SHA through HEAD",
    )
    args = parser.parse_args()

    if args.ci_range:
        if args.message_file is not None:
            parser.error("message_file cannot be used with --ci-range")
        return _check_ci_range()

    if args.message_file is None:
        parser.error("message_file is required unless --ci-range is used")

    try:
        message = args.message_file.read_text(encoding="utf-8")
    except (OSError, UnicodeError) as e:
        _print_error(f"failed to read commit message: {e}")
        return 1

    try:
        message = _clean_message_file(message)
    except ValueError as e:
        _print_error(f"failed to clean commit message: {e}")
        return 1

    return int(_report_message("commit message", message))


if __name__ == "__main__":
    sys.exit(main())
