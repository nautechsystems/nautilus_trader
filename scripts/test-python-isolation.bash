#!/usr/bin/env bash
set -euo pipefail

pkg_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")/../python" && pwd -P)"
# shellcheck source=scripts/native-path.bash
source "$pkg_dir/../scripts/native-path.bash"
neutral_dir="$(mktemp -d "${RUNNER_TEMP:-${TMPDIR:-/tmp}}/nautilus-python-isolation.XXXXXX")"
trap 'rm -rf "$neutral_dir"' EXIT
unset PYTHONPATH
unset VIRTUAL_ENV
unset UV_PROJECT_ENVIRONMENT
cd "$pkg_dir"
project_dir="$neutral_dir/python"
mkdir "$project_dir"
cp pyproject.toml uv.lock "$project_dir/"

project_dir_native="$(native_path "$project_dir")"
uv run --no-sync python - "$project_dir_native" << 'PY'
import importlib.machinery
import importlib.metadata
from pathlib import Path
import shutil
import sys
import sysconfig
import venv

package = Path.cwd() / "nautilus_trader"
if not any((package / ("_libnautilus" + suffix)).is_file()
           for suffix in importlib.machinery.EXTENSION_SUFFIXES):
    sys.exit("No built Python extension found; run make build-debug first")

project = Path(sys.argv[1])
context = venv.EnvBuilder(with_pip=False)
environment = project / ".venv"
context.create(environment)
site_packages = Path(context.ensure_directories(environment).lib_path)
shutil.copytree(package, site_packages / package.name,
                ignore=shutil.ignore_patterns("__pycache__"))

# Adding site-packages does not execute its .pth files, which expose editable source trees.
dependencies = dict.fromkeys(sysconfig.get_path(key) for key in ("purelib", "platlib"))
(site_packages / "dependencies.pth").write_text("\n".join(dependencies) + "\n", encoding="utf-8")

distribution = importlib.metadata.distribution("nautilus-trader")
for entry in distribution.files or ():
    if entry.name == "METADATA" and entry.parent.name.endswith(".dist-info"):
        metadata = Path(distribution.locate_file(entry)).parent
        shutil.copytree(metadata, site_packages / metadata.name)
        break
else:
    sys.exit("Installed nautilus-trader distribution metadata is missing")
PY

bash "$pkg_dir/../scripts/ci/check-python-isolation.bash" "$project_dir" "$pkg_dir" "$@"
