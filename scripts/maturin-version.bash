#!/usr/bin/env bash
set -euo pipefail

# Extract the exact maturin version from python/pyproject.toml.
#
# Usage: maturin-version.bash
# Example output: 1.14.0

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PYPROJECT="${SCRIPT_DIR}/../python/pyproject.toml"

if [[ ! -f "$PYPROJECT" ]]; then
  echo "Error: python/pyproject.toml not found at $PYPROJECT" >&2
  exit 1
fi

if [[ -n "${PYTHON:-}" ]] && command -v "$PYTHON" &> /dev/null; then
  PYTHON_CMD=("$PYTHON")
elif command -v python3 &> /dev/null; then
  PYTHON_CMD=(python3)
elif command -v python &> /dev/null; then
  PYTHON_CMD=(python)
elif command -v py &> /dev/null; then
  PYTHON_CMD=(py -3)
else
  echo "Error: No Python interpreter found (tried: \$PYTHON, python3, python, py)" >&2
  exit 1
fi

VERSION="$(
  "${PYTHON_CMD[@]}" - "$PYPROJECT" << 'PY'
import re
import sys
import tomllib
from pathlib import Path


pyproject = Path(sys.argv[1])
with pyproject.open("rb") as f:
    data = tomllib.load(f)

pattern = re.compile(r"^maturin==(?P<version>[A-Za-z0-9][A-Za-z0-9._+!-]*)$")


def maturin_pin(specs: list[str], label: str) -> str:
    for spec in specs:
        if spec.startswith("maturin"):
            match = pattern.fullmatch(spec)
            if match is None:
                raise SystemExit(
                    f"Error: {label} must pin maturin exactly as "
                    f"'maturin==<version>', found {spec!r}",
                )
            return match.group("version")

    raise SystemExit(f"Error: Could not find maturin in {label}")


build_version = maturin_pin(data["build-system"]["requires"], "build-system.requires")
dev_version = maturin_pin(data["dependency-groups"]["dev"], "dependency-groups.dev")

if build_version != dev_version:
    raise SystemExit(
        "Error: maturin versions differ between build-system.requires "
        f"({build_version}) and dependency-groups.dev ({dev_version})",
    )

print(build_version, end="")
PY
)"

if [[ -z "$VERSION" ]]; then
  echo "Error: Could not extract maturin version from $PYPROJECT" >&2
  exit 1
fi

echo -n "$VERSION"
