#!/usr/bin/env bash
set -euo pipefail

main() {
  script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd -P)"
  # shellcheck source=scripts/native-path.bash
  source "$script_dir/native-path.bash"
  case_root="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/nautilus-path.XXXXXX")"
  trap 'rm -rf "$case_root"' EXIT

  test_platforms
  test_errors
  test_native_tools
  echo 'Native path tests passed'
}

test_platforms() (
  local mock_platform path actual
  uname() { printf '%s\n' "$mock_platform"; }
  for mock_platform in Linux Darwin; do
    for path in '/tmp/path with spaces' '../wheel dir/package.whl' 'C:/already native'; do
      actual="$(native_path "$path")"
      [[ "$actual" == "$path" ]] || return 1
    done
  done

  cygpath() {
    [[ "$#" -eq 3 && "$1" == -m && "$2" == -- && "$3" == "$path" ]] || return 2
    printf '%s\n' 'C:/converted path'
  }
  for mock_platform in MINGW64_NT-10.0 MSYS_NT-10.0 CYGWIN_NT-10.0; do
    for path in '/c/wheel dir' '/cygdrive/c/wheel dir' '../wheel dir' 'C:\wheel dir' 'C:/wheel dir' '//server/share/wheel dir'; do
      actual="$(native_path "$path")"
      [[ "$actual" == 'C:/converted path' ]] || return 1
    done
  done
)

test_errors() (
  local status uname_status=0
  if native_path > "$case_root/output" 2>&1 ||
    native_path '' > "$case_root/output" 2>&1 ||
    native_path one two > "$case_root/output" 2>&1; then
    echo 'Invalid path arguments were accepted' >&2
    return 1
  fi

  uname() {
    echo MINGW64_NT-10.0
    return "$uname_status"
  }
  mkdir "$case_root/empty-bin"
  status=0
  PATH="$case_root/empty-bin" native_path /c/wheel > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq 1 ]] || return 1
  grep -Fq 'require cygpath' "$case_root/output"

  cygpath() { return 37; }
  status=0
  native_path /c/wheel > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq 37 ]] || return 1

  uname_status=38
  status=0
  native_path /c/wheel > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq 38 ]] || return 1
)

test_native_tools() (
  local wheel_dir wheel_path target_dir python
  mkdir "$case_root/wheel dir" "$case_root/installed dir"
  cd "$case_root"
  python="$(uv python find --system)"
  python="$(native_path "$python")"
  wheel_dir="$(native_path "$case_root/wheel dir")"
  target_dir="$(native_path "$case_root/installed dir")"
  "$python" - "$wheel_dir" "$(native_path 'wheel dir')" << 'PY'
from pathlib import Path
import sys
import zipfile

directory = Path(sys.argv[1])
assert directory.resolve() == (Path.cwd() / "wheel dir").resolve()
assert Path(sys.argv[2]).resolve() == directory.resolve()
wheel = directory / "path_probe-1.0-py3-none-any.whl"
with zipfile.ZipFile(wheel, "w") as archive:
    archive.writestr("path_probe.py", "VALUE = 37\n")
    archive.writestr("path_probe-1.0.dist-info/METADATA", (
        "Metadata-Version: 2.1\nName: path-probe\nVersion: 1.0\n"
        "Provides-Extra: visualization\n"
    ))
    archive.writestr("path_probe-1.0.dist-info/WHEEL", (
        "Wheel-Version: 1.0\nGenerator: path-probe\n"
        "Root-Is-Purelib: true\nTag: py3-none-any\n"
    ))
    archive.writestr("path_probe-1.0.dist-info/RECORD", "")
PY

  wheel_path="$(native_path "$case_root/wheel dir/path_probe-1.0-py3-none-any.whl")"
  uv pip install --python "$python" \
    --target "$target_dir" --no-deps --no-index --no-cache "${wheel_path}[visualization]"
  "$python" - "$target_dir" << 'PY'
from pathlib import Path
import sys

target = Path(sys.argv[1])
assert target.resolve() == (Path.cwd() / "installed dir").resolve()
assert (target / "path_probe.py").read_text(encoding="utf-8") == "VALUE = 37\n"
assert (target / "path_probe-1.0.dist-info/METADATA").is_file()
PY

  case "$(uname -s)" in
    MINGW* | MSYS* | CYGWIN*)
      [[ "$(native_path "$wheel_dir")" == "$wheel_dir" ]]
      [[ "$(native_path "${wheel_dir//\//\\}")" == "$wheel_dir" ]]
      ;;
  esac
)

main "$@"
