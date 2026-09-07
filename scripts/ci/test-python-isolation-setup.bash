#!/usr/bin/env bash
set -euo pipefail

unset PYTHONPATH
unset VIRTUAL_ENV
unset UV_PROJECT_ENVIRONMENT

repo_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd -P)
case_root=$(mktemp -d "${TMPDIR:-/tmp}/nautilus-isolation-setup.XXXXXX")
trap 'rm -rf "$case_root"' EXIT
mkdir -p "$case_root/source/scripts/ci" "$case_root/source/python" "$case_root/bin" "$case_root/temp"
cp "$repo_root/scripts/test-python-isolation.bash" "$case_root/source/scripts/"
cp "$repo_root/scripts/native-path.bash" "$case_root/source/scripts/"

python3 - "$case_root/source/python" << 'PY'
from pathlib import Path
import sys
import venv

project = Path(sys.argv[1])
venv.EnvBuilder(with_pip=False).create(project / ".venv")
PY

cat > "$case_root/bin/uv" << 'MOCK'
#!/usr/bin/env bash
set -euo pipefail
[[ "$1 $2 $3" == 'run --no-sync python' ]] || exit 90
[[ -z "${UV_PROJECT_ENVIRONMENT:-}" && -z "${PYTHONPATH:-}" && -z "${VIRTUAL_ENV:-}" ]] || exit 91
shift 3
if [ -x .venv/bin/python ]; then
  exec .venv/bin/python "$@"
fi
exec .venv/Scripts/python.exe "$@"
MOCK
chmod +x "$case_root/bin/uv"

(
  cd "$case_root/source/python"
  PATH="$case_root/bin:$PATH" uv run --no-sync python - << 'PY'
import importlib.machinery
from pathlib import Path
import sysconfig

project = Path.cwd()
package = project / "nautilus_trader"
package.mkdir()
(package / "__init__.py").write_text("VALUE = 37\n", encoding="utf-8")
(package / ("_libnautilus" + importlib.machinery.EXTENSION_SUFFIXES[0])).touch()
(project / "tests").mkdir()
(project / "tests/__init__.py").touch()
for name in ("pyproject.toml", "uv.lock"):
    (project / name).touch()
site = Path(sysconfig.get_path("purelib"))
(site / "editable.pth").write_text(str(project) + "\n", encoding="utf-8")
(site / "dependency_example.py").write_text("VALUE = 83\n", encoding="utf-8")
metadata = site / "nautilus_trader-1.0.dist-info"
metadata.mkdir()
(metadata / "METADATA").write_text("Name: nautilus-trader\nVersion: 1.0\n", encoding="utf-8")
(metadata / "RECORD").write_text("nautilus_trader-1.0.dist-info/METADATA,,\n", encoding="utf-8")
PY
)

cat > "$case_root/source/scripts/ci/check-python-isolation.bash" << 'CHECK'
#!/usr/bin/env bash
set -euo pipefail
cd "$1"
if [ -x .venv/bin/python ]; then
  python=.venv/bin/python
else
  python=.venv/Scripts/python.exe
fi
"$python" - <<'PY'
import importlib.metadata
import importlib.util
from pathlib import Path
import subprocess
import sys
import dependency_example
import nautilus_trader

assert nautilus_trader.VALUE == 37
assert dependency_example.VALUE == 83
assert importlib.metadata.version("nautilus-trader") == "1.0"
assert Path(nautilus_trader.__file__).is_relative_to(sys.prefix)
assert importlib.util.find_spec("tests") is None
result = subprocess.run([sys.executable, "-c", "import tests"], capture_output=True, text=True)
assert result.returncode == 1
assert "ModuleNotFoundError: No module named 'tests'" in result.stderr
PY
CHECK

PATH="$case_root/bin:$PATH" RUNNER_TEMP="$case_root/temp" \
  PYTHONPATH=contaminated VIRTUAL_ENV=contaminated UV_PROJECT_ENVIRONMENT=contaminated \
  bash "$case_root/source/scripts/test-python-isolation.bash"
[[ -z "$(ls -A "$case_root/temp")" ]] || {
  echo 'Isolation left temporary files' >&2
  exit 1
}
[[ -f "$case_root/source/python/.venv/pyvenv.cfg" ]] || exit 1

rm "$case_root/source/python/nautilus_trader/"_libnautilus*
status=0
PATH="$case_root/bin:$PATH" RUNNER_TEMP="$case_root/temp" \
  bash "$case_root/source/scripts/test-python-isolation.bash" > "$case_root/missing.log" 2>&1 || status=$?
[[ "$status" -eq 1 ]] || {
  echo 'Missing extension was not rejected' >&2
  exit 1
}
grep -Fq 'No built Python extension found; run make build-debug first' "$case_root/missing.log"
[[ -z "$(ls -A "$case_root/temp")" ]] || exit 1

echo 'Python isolation setup tests passed'
