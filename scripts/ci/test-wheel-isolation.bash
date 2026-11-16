#!/usr/bin/env bash
set -euo pipefail

main() {
  repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
  real_uv=$(command -v uv)
  case_root=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-wheel-test.XXXXXX")
  trap 'rm -rf "$case_root"' EXIT

  mkdir -p "$case_root/bin" "$case_root/source/python/.venv" \
    "$case_root/source/dist" "$case_root/source/scripts/ci" "$case_root/source/examples" \
    "$case_root/temp"
  cp "$repo_root/scripts/ci/test-wheel.bash" \
    "$repo_root/scripts/ci/test-python-doctests.bash" \
    "$repo_root/scripts/ci/check-python-isolation.bash" \
    "$repo_root/scripts/ci/check-python-types.bash" "$case_root/source/scripts/ci/"
  cp "$repo_root/scripts/native-path.bash" "$case_root/source/scripts/"
  printf '%s\n' 'development environment' > "$case_root/source/python/.venv/sentinel"
  touch "$case_root/source/dist/package.whl" "$case_root/source/python/pyproject.toml" \
    "$case_root/source/python/uv.lock"

  cat > "$case_root/bin/uv" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=scripts/native-path.bash
source "$CASE_ROOT/source/scripts/native-path.bash"
case_root_native="$(native_path "$CASE_ROOT")"

[[ -z "${PYTHONPATH:-}" && -z "${VIRTUAL_ENV:-}" ]] || exit 80
if [[ "$1 $2" == 'python find' ]]; then
  [[ -z "${UV_PROJECT_ENVIRONMENT:-}" ]] || exit 87
  if [[ -n "${REAL_UV:-}" ]]; then
    exec "$REAL_UV" "$@"
  fi
  if [[ -n "${UV_PYTHON:-}" ]]; then
    [[ "$#" -eq 3 && "$3" == "$UV_PYTHON" ]] || exit 94
  else
    [[ "$#" -eq 2 ]] || exit 94
  fi
  echo "$case_root_native/source/python/.venv/bin/python"
  exit 0
fi
[[ -z "${UV_PROJECT_ENVIRONMENT:-}" ]] || exit 81
printf '%s|%s\n' "$PWD" "$*" >> "$CASE_ROOT/uv.log"
if [[ "$1" != pip ]]; then
  [[ "$2" == --project && "$3" == "$case_root_native"/temp/nautilus-wheel.*/python ]] || exit 88
  environment="$CASE_ROOT${3#"$case_root_native"}/.venv"
  environment_native="$3/.venv"
fi

case "$1" in
  sync)
    expected_python=${EXPECTED_PYTHON:-"$case_root_native/source/python/.venv/bin/python"}
    [[ "$*" == "sync --project $3 --python $expected_python --frozen --group test --extra visualization --no-install-package nautilus-trader" ]] || exit 82
    mkdir -p "$environment"
    ;;
  pip)
    [[ "$3" == '--python' && "$4" == "$case_root_native"/temp/nautilus-wheel.*/python/.venv/bin/python ]] || exit 83
    if [[ "$5" == --reinstall ]]; then
      [[ "$6" == --no-deps && "$7" == "$case_root_native/source/dist/package.whl[visualization]" ]] || exit 89
    fi
    ;;
  run)
    if [[ "$*" == *'python -m pytest'* ]]; then
      [[ "${PYTHONWARNDEFAULTENCODING:-}" == 1 ]] || exit 92
      [[ "${PYTHONWARNINGS:-}" == *error::EncodingWarning,ignore::EncodingWarning:plotly.validator_cache ]] || exit 93
    fi
    case "$*" in
      *'print(sys.executable)'*)
        echo "$environment_native/bin/python"
        ;;
      *'print(Path.cwd().resolve().parent)'*)
        echo "$case_root_native/source"
        ;;
      *)
        [[ "$PWD" == "$CASE_ROOT"/temp/nautilus-* ]] || exit 84
        [[ "$TEST_DATA_ROOT_PATH" == "$case_root_native/source" ]] || exit 85
        if [[ "$*" == *'package_dir.is_relative_to(environment_dir)'* ]]; then
          [[ "${FAIL_STAGE:-}" != origin ]] || exit 42
        elif [[ "$*" == *'--import-mode=importlib'* ]]; then
          [[ "${FAIL_STAGE:-}" != pytest ]] || exit 43
        fi
        ;;
    esac
    ;;
  *) exit 86 ;;
esac
MOCK
  chmod +x "$case_root/bin/uv"

  run_checks
  check_interpreter_selection
  for python_request in 3.13 3.15 'C:/Python 3.15/python.exe'; do
    run_case success 0 "$python_request"
  done
  case "$(uname -s)" in
    Linux | Darwin)
      cat > "$case_root/bin/uname" << 'MOCK'
#!/usr/bin/env bash
echo MINGW64_NT-10.0
MOCK
      cat > "$case_root/bin/cygpath" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$#" -eq 3 && "$1" == -m && "$2" == -- ]] || exit 90
printf 'C:%s\n' "${3#C:}"
MOCK
      chmod +x "$case_root/bin/uname" "$case_root/bin/cygpath"
      run_checks
      ;;
  esac
  rm "$case_root/source/dist/package.whl"
  run_case missing 1
  [[ ! -s "$case_root/uv.log" ]] || fail 'Missing wheel still created an environment'
  touch "$case_root/source/dist/first.whl" "$case_root/source/dist/second.whl"
  run_case ambiguous 1
  [[ ! -s "$case_root/uv.log" ]] || fail 'Ambiguous wheels still created an environment'

  echo 'Wheel isolation script tests passed'
}

check_interpreter_selection() {
  local selected_python
  selected_python=$(
    python3 - "$case_root" << 'PY'
from pathlib import Path
import os
import sys
import venv

root = Path(sys.argv[1])
environment = root / "selected python" / ".venv"
venv.EnvBuilder(with_pip=False).create(environment)
project = root / "source" / "python"
(project / ".python-version").write_text(
    f"{sys.version_info.major}.{sys.version_info.minor}\n", encoding="utf-8",
)
(project / "pyproject.toml").write_text(
    '[project]\nname = "wheel-test"\nversion = "0.0.0"\nrequires-python = ">=3.10"\n',
    encoding="utf-8",
)
print(environment / ("Scripts/python.exe" if os.name == "nt" else "bin/python"), end="")
PY
  )
  REAL_UV="$real_uv" UV_OFFLINE=1 UV_PYTHON_DOWNLOADS=never UV_CACHE_DIR="$case_root/uv-cache" \
    run_case success 0 "$selected_python" "$selected_python"
  REAL_UV="$real_uv" UV_OFFLINE=1 UV_PYTHON_DOWNLOADS=never UV_CACHE_DIR="$case_root/uv-cache" \
    run_case invalid-interpreter 2 "$case_root/missing-python"
}

run_checks() {
  run_case success 0
  grep -Fq -- '--import-mode=importlib' "$case_root/uv.log" || fail 'Missing isolated pytest run'
  grep -Fq -- '--doctest-modules' "$case_root/uv.log" || fail 'Missing wheel doctests'
  grep -Fq -- 'ty check' "$case_root/uv.log" || fail 'Missing wheel type checks'
  run_case origin 42
  if grep -Fq -- '--import-mode=importlib' "$case_root/uv.log"; then
    fail 'Tests ran after the package origin check failed'
  fi
  run_case pytest 43
  if grep -Fq -- '--doctest-modules' "$case_root/uv.log"; then
    fail 'Doctests ran after pytest failed'
  fi
}

run_case() {
  local stage="$1"
  local expected="$2"
  local python_request=${3:-}
  local status=0
  : > "$case_root/uv.log"
  (
    cd "$case_root/source/python"
    PATH="$case_root/bin:$PATH" CASE_ROOT="$case_root" RUNNER_TEMP="$case_root/temp" \
      FAIL_STAGE="$stage" PYTHONPATH=contaminated VIRTUAL_ENV=contaminated \
      UV_PROJECT_ENVIRONMENT=contaminated UV_PYTHON="$python_request" \
      EXPECTED_PYTHON="${4:-}" bash ../scripts/ci/test-wheel.bash
  ) > "$case_root/output" 2>&1 || status=$?
  [[ "$status" -eq "$expected" ]] || {
    cat "$case_root/output" >&2
    fail "$stage returned $status; expected $expected"
  }
  [[ "$(cat "$case_root/source/python/.venv/sentinel")" == 'development environment' ]] ||
    fail 'Wheel validation changed the development environment'
  [[ -z "$(ls -A "$case_root/temp")" ]] || fail 'Wheel validation left a temporary environment'
}

fail() {
  echo "ERROR: $1" >&2
  exit 1
}

main "$@"
