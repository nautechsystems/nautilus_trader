import subprocess
import sys

from setuptools import build_meta as _orig


def get_requires_for_build_wheel(config_settings=None):
    return _orig.get_requires_for_build_wheel(config_settings)


def get_requires_for_build_sdist(config_settings=None):
    return _orig.get_requires_for_build_sdist(config_settings)


def build_editable(
    wheel_directory,
    config_settings=None,
    metadata_directory=None,
):
    _run_build()
    return _orig.build_editable(
        wheel_directory,
        config_settings=config_settings,
        metadata_directory=metadata_directory,
    )


def build_sdist(sdist_directory, config_settings=None):
    _run_build()
    return _orig.build_sdist(sdist_directory, config_settings)


def build_wheel(wheel_directory, config_settings=None, metadata_directory=None):
    _run_build()
    return _orig.build_wheel(wheel_directory, config_settings, metadata_directory)


def _run_build() -> None:
    subprocess.run(
        [sys.executable, "-u", "build.py"],
        check=True,
        stdout=sys.stdout,
        stderr=sys.stderr,
    )
