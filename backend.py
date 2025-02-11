import re
import subprocess
import sys
from pathlib import Path

from packaging.tags import sys_tags
from setuptools import Distribution
from setuptools import build_meta as _orig
from setuptools.command.bdist_wheel import bdist_wheel as _bdist_wheel


def get_requires_for_build_wheel(config_settings=None):
    return _orig.get_requires_for_build_wheel(config_settings)


def get_requires_for_build_sdist(config_settings=None):
    return _orig.get_requires_for_build_sdist(config_settings)


def build_editable(wheel_directory, config_settings=None, metadata_directory=None):
    _run_build()
    return _orig.build_editable(wheel_directory, config_settings, metadata_directory)


def build_sdist(sdist_directory, config_settings=None):
    _run_build()
    return _orig.build_sdist(sdist_directory, config_settings)


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    _run_build()
    return _build_wheel_custom_tags(wheel_directory, config_settings, metadata_directory)


class BDistWheelCmd(_bdist_wheel):
    """
    Custom wheel command that overrides the default wheel tags.
    """

    def get_tag(self):
        best_tag = next(sys_tags())
        python_tag = best_tag.interpreter  # e.g., "cp312"
        abi_tag = best_tag.abi  # e.g., "cp312"
        plat_name = best_tag.platform  # e.g., "macosx_14_0_arm64"
        return python_tag, abi_tag, plat_name


def _build_wheel_custom_tags(wheel_directory, config_settings, metadata_directory):
    dist = Distribution(
        {
            "name": "nautilus_trader",
            "version": _get_nautilus_version(),
            "packages": ["nautilus_trader"],
            "package_dir": {"nautilus_trader": "nautilus_trader"},
            "include_package_data": True,
            "cmdclass": {"bdist_wheel": BDistWheelCmd},
        },
    )

    # Create and configure the bdist_wheel command object
    dist.script_name = "build.py"
    cmd_obj = dist.get_command_obj("bdist_wheel")
    cmd_obj.dist_dir = wheel_directory

    if metadata_directory:
        cmd_obj.keep_temp = True

    cmd_obj.ensure_finalized()
    cmd_obj.run()

    # Now the wheel should be in wheel_directory
    wheel_files = list(Path(wheel_directory).glob("*.whl"))
    if not wheel_files:
        raise RuntimeError("No wheel was produced by bdist_wheel.")
    return str(wheel_files[0])


def _run_build():
    subprocess.run(
        [sys.executable, "-u", "build.py"],
        check=True,
        stdout=sys.stdout,
        stderr=sys.stderr,
    )


def _get_nautilus_version():
    content = Path("pyproject.toml").read_text(encoding="utf-8")
    match = re.search(r'version\s*=\s*"([^"]+)"', content)
    if not match:
        raise RuntimeError("Could not find version in pyproject.toml.")
    return match.group(1)
