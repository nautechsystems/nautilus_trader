#!/usr/bin/env python3

import datetime as dt
import itertools
import os
import platform
import re
import shutil
import subprocess
import sys
import sysconfig
from pathlib import Path

import numpy as np
from Cython.Build import build_ext
from Cython.Build import cythonize
from Cython.Compiler import Options
from Cython.Compiler.Version import version as cython_compiler_version
from packaging.version import Version
from setuptools import Distribution
from setuptools import Extension


# Platform constants
IS_LINUX = platform.system() == "Linux"
IS_MACOS = platform.system() == "Darwin"
IS_WINDOWS = platform.system() == "Windows"
IS_ARM64 = platform.machine() in ("arm64", "aarch64")


# The Rust toolchain to use for builds
RUSTUP_TOOLCHAIN = os.getenv("RUSTUP_TOOLCHAIN", "stable")
# The Cargo build mode
BUILD_MODE = os.getenv("BUILD_MODE", "release")
# If PROFILE_MODE mode is enabled, include traces necessary for coverage and profiling
PROFILE_MODE = bool(os.getenv("PROFILE_MODE", ""))
# If ANNOTATION mode is enabled, generate an annotated HTML version of the input source files
ANNOTATION_MODE = bool(os.getenv("ANNOTATION_MODE", ""))
# If PARALLEL build is enabled, uses all CPUs for compile stage of build
PARALLEL_BUILD = os.getenv("PARALLEL_BUILD", "true").lower() == "true"
# If COPY_TO_SOURCE is enabled, copy built *.so files back into the source tree
COPY_TO_SOURCE = os.getenv("COPY_TO_SOURCE", "true").lower() == "true"
# Force stripping of debug symbols even in non-release builds
FORCE_STRIP = os.getenv("FORCE_STRIP", "false").lower() == "true"
# If PyO3 only then don't build C extensions to reduce compilation time
PYO3_ONLY = os.getenv("PYO3_ONLY", "").lower() != ""
# If dry run only print the commands that would be executed
DRY_RUN = bool(os.getenv("DRY_RUN", ""))

# Precision mode configuration
# https://nautilustrader.io/docs/nightly/getting_started/installation#precision-mode
HIGH_PRECISION = os.getenv("HIGH_PRECISION", "true").lower() == "true"
if IS_WINDOWS and HIGH_PRECISION:
    print(
        "Warning: high-precision mode not supported on Windows (128-bit integers unavailable)\nForcing standard-precision (64-bit) mode",
    )
    HIGH_PRECISION = False

if PROFILE_MODE:
    # For subsequent debugging, the C source needs to be in the same tree as
    # the Cython code (not in a separate build directory).
    BUILD_DIR = None
elif ANNOTATION_MODE:
    BUILD_DIR = "build/annotated"
else:
    BUILD_DIR = "build/optimized"

################################################################################
#  RUST BUILD
################################################################################

USE_SCCACHE = "sccache" in os.environ.get("CC", "") or "sccache" in os.environ.get("CXX", "")

if IS_LINUX:
    # Use clang as the default compiler
    os.environ["CC"] = "sccache clang" if USE_SCCACHE else "clang"
    os.environ["CXX"] = "sccache clang++" if USE_SCCACHE else "clang++"
    os.environ["LDSHARED"] = "clang -shared"

if IS_MACOS and IS_ARM64:
    os.environ["CFLAGS"] = f"{os.environ.get('CFLAGS', '')} -arch arm64"
    os.environ["LDFLAGS"] = f"{os.environ.get('LDFLAGS', '')} -arch arm64 -w"

if IS_LINUX and IS_ARM64:
    os.environ["CFLAGS"] = f"{os.environ.get('CFLAGS', '')} -fPIC"
    os.environ["LDFLAGS"] = f"{os.environ.get('LDFLAGS', '')} -fPIC"

    python_lib_dir = os.environ.get("PYTHON_LIB_DIR")
    python_version = ".".join(platform.python_version_tuple()[:2])  # e.g. "3.12"

    if python_lib_dir:
        print(f"Setting RUSTFLAGS to link with Python {python_version} in {python_lib_dir}")
        rustflags = f"{os.environ.get('RUSTFLAGS', '')} -C link-arg=-L{python_lib_dir} -C link-arg=-lpython{python_version}"
        os.environ["RUSTFLAGS"] = rustflags

if IS_WINDOWS:
    # Linker error 1181
    # https://docs.microsoft.com/en-US/cpp/error-messages/tool-errors/linker-tools-error-lnk1181?view=msvc-170&viewFallbackFrom=vs-2019
    RUST_LIB_PFX = ""
    RUST_STATIC_LIB_EXT = "lib"
    RUST_DYLIB_EXT = "dll"
elif IS_MACOS:
    RUST_LIB_PFX = "lib"
    RUST_STATIC_LIB_EXT = "a"
    RUST_DYLIB_EXT = "dylib"
else:  # Linux
    RUST_LIB_PFX = "lib"
    RUST_STATIC_LIB_EXT = "a"
    RUST_DYLIB_EXT = "so"

CARGO_TARGET_DIR = os.environ.get("CARGO_TARGET_DIR", Path.cwd() / "target")
CARGO_BUILD_TARGET = os.environ.get("CARGO_BUILD_TARGET", "")

# Determine the profile directory name
if BUILD_MODE == "release":
    profile_dir = "release"
elif BUILD_MODE == "debug-pyo3":
    profile_dir = "debug-pyo3"
else:
    profile_dir = "debug"

CARGO_TARGET_DIR = Path(CARGO_TARGET_DIR) / CARGO_BUILD_TARGET / profile_dir

# Directories with headers to include
RUST_INCLUDES = ["nautilus_trader/core/includes"]
RUST_LIB_PATHS: list[Path] = [
    CARGO_TARGET_DIR / f"{RUST_LIB_PFX}nautilus_backtest.{RUST_STATIC_LIB_EXT}",
    CARGO_TARGET_DIR / f"{RUST_LIB_PFX}nautilus_common.{RUST_STATIC_LIB_EXT}",
    CARGO_TARGET_DIR / f"{RUST_LIB_PFX}nautilus_core.{RUST_STATIC_LIB_EXT}",
    CARGO_TARGET_DIR / f"{RUST_LIB_PFX}nautilus_model.{RUST_STATIC_LIB_EXT}",
    CARGO_TARGET_DIR / f"{RUST_LIB_PFX}nautilus_persistence.{RUST_STATIC_LIB_EXT}",
]
RUST_LIBS: list[str] = [str(path) for path in RUST_LIB_PATHS]


def _set_feature_flags() -> list[str]:
    features = "cython-compat,ffi,python,extension-module,postgres"
    flags = ["--no-default-features", "--features"]

    if HIGH_PRECISION:
        features += ",high-precision"

    flags.append(features)

    return flags


def _build_rust_libs() -> None:
    print("Compiling Rust libraries...")

    try:
        # Build the Rust libraries using Cargo
        if RUSTUP_TOOLCHAIN not in ("stable", "nightly"):
            raise ValueError(f"Invalid `RUSTUP_TOOLCHAIN` '{RUSTUP_TOOLCHAIN}'")

        needed_crates = [
            "nautilus-backtest",
            "nautilus-common",
            "nautilus-core",
            "nautilus-infrastructure",
            "nautilus-model",
            "nautilus-persistence",
            "nautilus-pyo3",
        ]

        if BUILD_MODE == "release":
            build_options = ["--release"]
            # Only pass '-s' at link time on Linux. On macOS this flag is obsolete
            # and may cause failures with recent toolchains. Cargo already performs
            # symbol stripping per profile, and we post-strip where applicable.
            if IS_LINUX:
                existing_rustflags = os.environ.get("RUSTFLAGS", "")
                os.environ["RUSTFLAGS"] = f"{existing_rustflags} -C link-arg=-s"
        elif BUILD_MODE == "debug-pyo3":
            build_options = ["--profile", "debug-pyo3"]
        else:
            build_options = []

        features = _set_feature_flags()

        cmd_args = [
            "cargo",
            "build",
            "--lib",
            *itertools.chain.from_iterable(("-p", p) for p in needed_crates),
            *build_options,
            *features,
        ]

        if RUSTUP_TOOLCHAIN == "nightly":
            cmd_args.insert(1, "+nightly")

        print(" ".join(cmd_args))

        subprocess.run(
            cmd_args,
            check=True,
        )
    except subprocess.CalledProcessError as e:
        raise RuntimeError(
            f"Error running cargo: {e}",
        ) from e


################################################################################
#  CYTHON BUILD
################################################################################
# https://cython.readthedocs.io/en/latest/src/userguide/source_files_and_compilation.html

Options.docstrings = True  # Include docstrings in modules
Options.fast_fail = True  # Abort the compilation on the first error occurred
Options.annotate = ANNOTATION_MODE  # Create annotated HTML files for each .pyx
if ANNOTATION_MODE:
    Options.annotate_coverage_xml = "coverage.xml"

CYTHON_COMPILER_DIRECTIVES = {
    "language_level": "3",
    "cdivision": True,  # If division is as per C with no check for zero (35% speed up)
    "nonecheck": True,  # Insert extra check for field access on C extensions
    "embedsignature": True,  # If docstrings should be embedded into C signatures
    "profile": PROFILE_MODE,  # If we're debugging or profiling
    "linetrace": PROFILE_MODE,  # If we're debugging or profiling
    "warn.maybe_uninitialized": True,
}

# TODO: Temporarily separate Cython configuration while we require v3.0.11 for coverage
if Version(cython_compiler_version) >= Version("3.1.2"):
    Options.warning_errors = True  # Treat compiler warnings as errors
    Options.extra_warnings = True
    CYTHON_COMPILER_DIRECTIVES["warn.deprecated.IF"] = False


def _build_extensions() -> list[Extension]:
    # Regarding the compiler warning: #warning "Using deprecated NumPy API,
    # disable it with " "#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION"
    # https://stackoverflow.com/questions/52749662/using-deprecated-numpy-api
    # From the Cython docs: "For the time being, it is just a warning that you can ignore."
    define_macros: list[tuple[str, str | None]] = [
        ("NPY_NO_DEPRECATED_API", "NPY_1_7_API_VERSION"),
    ]
    if PROFILE_MODE or ANNOTATION_MODE:
        # Profiling requires special macro directives
        define_macros.append(("CYTHON_TRACE", "1"))

    extra_compile_args = []
    extra_link_args = RUST_LIBS

    if not IS_WINDOWS:
        # Suppress warnings produced by Cython boilerplate
        extra_compile_args.append("-Wno-unreachable-code")
        if BUILD_MODE == "release":
            extra_compile_args.append("-O2")
            extra_compile_args.append("-pipe")

            if IS_LINUX:
                extra_compile_args.append("-ffunction-sections")
                extra_compile_args.append("-fdata-sections")
                extra_link_args.append("-Wl,--gc-sections")
                extra_link_args.append("-Wl,--as-needed")
                # Ensure non-executable stack on Linux to avoid loader errors
                # when any input object accidentally requests an execstack.
                extra_link_args.append("-Wl,-z,noexecstack")

    if IS_WINDOWS:
        # Standard Windows system libraries required when linking Cython extensions.
        # Keep this list lowercase and alphabetically sorted for easy maintenance
        # and to avoid duplicates sneaking in.
        extra_link_args += [
            "advapi32.lib",
            "bcrypt.lib",
            "crypt32.lib",
            "iphlpapi.lib",
            "kernel32.lib",
            "ncrypt.lib",
            "netapi32.lib",
            "ntdll.lib",
            "ole32.lib",
            "oleaut32.lib",
            "pdh.lib",
            "powrprof.lib",
            "propsys.lib",
            "psapi.lib",
            "runtimeobject.lib",
            "schannel.lib",
            "secur32.lib",
            "shell32.lib",
            "user32.lib",
            "userenv.lib",
            "ws2_32.lib",
        ]

    print("Creating C extension modules...")
    print(f"define_macros={define_macros}")
    print(f"extra_compile_args={extra_compile_args}")

    return [
        Extension(
            name=str(pyx.relative_to(".")).replace(os.path.sep, ".")[:-4],
            sources=[str(pyx)],
            include_dirs=[np.get_include(), *RUST_INCLUDES],
            define_macros=define_macros,
            language="c",
            extra_link_args=extra_link_args,
            extra_compile_args=extra_compile_args,
        )
        for pyx in itertools.chain(Path("nautilus_trader").rglob("*.pyx"))
    ]


def _build_distribution(extensions: list[Extension]) -> Distribution:
    nthreads = os.cpu_count() or 1
    if IS_WINDOWS:
        nthreads = min(nthreads, 60)
    print(f"nthreads={nthreads}")

    distribution = Distribution(
        {
            "name": "nautilus_trader",
            "ext_modules": cythonize(
                module_list=extensions,
                compiler_directives=CYTHON_COMPILER_DIRECTIVES,
                nthreads=nthreads,
                build_dir=BUILD_DIR,
                gdb_debug=PROFILE_MODE,
            ),
            "zip_safe": False,
        },
    )
    return distribution


def _copy_build_dir_to_project(cmd: build_ext) -> None:
    # Copy built extensions back to the project tree
    for output in cmd.get_outputs():
        relative_extension = Path(output).relative_to(cmd.build_lib)
        if not Path(output).exists():
            continue

        # Copy the file and set permissions
        shutil.copyfile(output, relative_extension)
        mode = relative_extension.stat().st_mode
        mode |= (mode & 0o444) >> 2
        relative_extension.chmod(mode)

    print("Copied all compiled dynamic library files into source")


def _copy_rust_dylibs_to_project() -> None:
    # https://pyo3.rs/latest/building-and-distribution#manual-builds
    ext_suffix = sysconfig.get_config_var("EXT_SUFFIX")
    src = Path(CARGO_TARGET_DIR) / f"{RUST_LIB_PFX}nautilus_pyo3.{RUST_DYLIB_EXT}"
    dst = Path("nautilus_trader/core") / f"nautilus_pyo3{ext_suffix}"
    shutil.copyfile(src=src, dst=dst)

    print(f"Copied {src} to {dst}")


def _get_nautilus_version() -> str:
    with open("pyproject.toml", encoding="utf-8") as f:
        pyproject_content = f.read().strip()
    if not pyproject_content:
        raise ValueError("pyproject.toml is empty or not properly formatted")

    version_match = re.search(r'version\s*=\s*"(.*?)"', pyproject_content)
    if not version_match:
        raise ValueError("Version not found in pyproject.toml")

    return version_match.group(1)


def _get_clang_version() -> str:
    try:
        result = subprocess.run(
            ["clang", "--version"],  # noqa
            check=True,
            capture_output=True,
        )
        output = result.stdout.decode().splitlines()[0].lstrip("Apple ").lstrip("Ubuntu ").lstrip("clang version ")
        return output
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        err_msg = str(e) if isinstance(e, FileNotFoundError) else e.stderr.decode()
        raise RuntimeError(
            f"You are installing from source which requires the Clang compiler to be installed.\nError running clang: {err_msg}",
        ) from e


def _get_rustc_version() -> str:
    try:
        result = subprocess.run(
            ["rustc", "--version"],  # noqa
            check=True,
            capture_output=True,
        )
        output = result.stdout.decode().lstrip("rustc ").strip()
        return output
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        err_msg = str(e) if isinstance(e, FileNotFoundError) else e.stderr.decode()
        raise RuntimeError(
            "You are installing from source which requires the Rust compiler to be installed.\n"
            "Find more information at https://www.rust-lang.org/tools/install\n"
            f"Error running rustc: {err_msg}",
        ) from e


def _ensure_windows_python_import_lib() -> None:
    """
    Ensure that the *t* suffixed Python import library exists on Windows.

    On some official CPython Windows builds the import library is named
    ``pythonXY.lib`` (for example ``python313.lib``). However, when building
    C-extensions ``distutils``/``setuptools`` may ask the MSVC linker for the
    file ``pythonXYt.lib`` - note the additional *t* suffix. The *t* variant
    historically referred to a *thread-safe* build but is no longer shipped.

    When the file is missing the linker exits with
    ``LINK : fatal error LNK1104: cannot open file 'pythonXYt.lib'`` which
    breaks the CI build on Windows. To work around this we simply create a
    copy of the existing import library with the expected name **before** the
    extension build starts.

    """
    if not IS_WINDOWS:
        return

    try:
        # The virtual environment as well as the base installation may both
        # participate in the link search path.  Attempt the fix in both
        # locations to maximise the chance of success.
        candidate_roots = {Path(sys.base_prefix), Path(sys.prefix)}

        # Example: for Python 3.13 -> '313'
        major, minor, *_ = platform.python_version_tuple()
        version_compact = f"{major}{minor}"

        for root in candidate_roots:
            libs_dir = root / "libs"
            if not libs_dir.exists():
                continue

            src = libs_dir / f"python{version_compact}.lib"
            dst = libs_dir / f"python{version_compact}t.lib"

            if src.exists() and not dst.exists():
                print(
                    f"Creating missing Windows import lib {dst} (copying from {src})",
                )
                shutil.copyfile(src, dst)
    except Exception as e:  # pragma: no cover - defensive
        # Never fail the build because of this helper, just show the warning
        print(f"Warning: failed to create *t* suffixed Python import library: {e}")


def _strip_unneeded_symbols() -> None:
    try:
        print("Stripping unneeded symbols from binaries...")
        total_before = 0
        total_after = 0

        for so in itertools.chain(Path("nautilus_trader").rglob("*.so")):
            size_before = so.stat().st_size
            total_before += size_before

            if IS_LINUX:
                strip_cmd = ["strip", "--strip-all", "-R", ".comment", "-R", ".note", so]
            elif IS_MACOS:
                strip_cmd = ["strip", "-x", so]
            else:
                raise RuntimeError(f"Cannot strip symbols for platform {platform.system()}")
            subprocess.run(
                strip_cmd,  # type: ignore [arg-type]
                check=True,
                capture_output=True,
            )

            size_after = so.stat().st_size
            total_after += size_after

        if total_before > 0:
            reduction = (1 - total_after / total_before) * 100
            print(
                f"Stripped binaries: {total_before / 1024 / 1024:.1f}MB -> {total_after / 1024 / 1024:.1f}MB ({reduction:.1f}% reduction)",
            )
    except subprocess.CalledProcessError as e:
        raise RuntimeError(f"Error when stripping symbols.\n{e}") from e


def show_rustanalyzer_settings() -> None:
    """
    Show appropriate vscode settings for the build.
    """
    import json

    # Set environment variables
    settings: dict[str, object] = {}
    for key in [
        "rust-analyzer.check.extraEnv",
        "rust-analyzer.runnables.extraEnv",
        "rust-analyzer.cargo.features",
    ]:
        settings[key] = {
            "CC": os.environ["CC"],
            "CXX": os.environ["CXX"],
            "VIRTUAL_ENV": os.environ["VIRTUAL_ENV"],
        }

    # Set features
    features = _set_feature_flags()
    if features[0] == "--all-features":
        settings["rust-analyzer.cargo.features"] = "all"
        settings["rust-analyzer.check.features"] = "all"
    else:
        settings["rust-analyzer.cargo.features"] = features[1].split(",")
        settings["rust-analyzer.check.features"] = features[1].split(",")

    print("Set these rust analyzer settings in .vscode/settings.json")
    print(json.dumps(settings, indent=2))


def build() -> None:
    """
    Construct the extensions and distribution.
    """
    _ensure_windows_python_import_lib()
    _build_rust_libs()
    # Allow skipping Rust dylib copy in constrained environments
    if not os.getenv("SKIP_RUST_DYLIB_COPY"):
        _copy_rust_dylibs_to_project()

    if not PYO3_ONLY:
        # Create C Extensions to feed into cythonize()
        extensions = _build_extensions()
        distribution = _build_distribution(extensions)

        # Build and run the command
        print("Compiling C extension modules...")
        cmd: build_ext = build_ext(distribution)
        if PARALLEL_BUILD:
            cmd.parallel = os.cpu_count()
        cmd.ensure_finalized()
        cmd.run()

        if COPY_TO_SOURCE:
            # Copy the build back into the source tree for development and wheel packaging
            _copy_build_dir_to_project(cmd)

    if (BUILD_MODE == "release" or FORCE_STRIP) and (IS_LINUX or IS_MACOS):
        # Strip symbols for release builds or when forced
        _strip_unneeded_symbols()


def print_env_var_if_exists(key: str) -> None:
    value = os.environ.get(key)
    if value is not None:
        print(f"{key}={value}")


if __name__ == "__main__":
    print("\033[36m")
    print("=====================================================================")
    print(f"Nautilus Builder {_get_nautilus_version()}")
    print("=====================================================================\033[0m")
    print(f"System: {platform.system()} {platform.machine()}")
    print(f"Clang:  {_get_clang_version()}")
    print(f"Rust:   {_get_rustc_version()}")
    print(f"Python: {platform.python_version()} ({sys.executable})")
    print(f"Cython: {cython_compiler_version}")
    print(f"NumPy:  {np.__version__}")

    print(f"\nRUSTUP_TOOLCHAIN={RUSTUP_TOOLCHAIN}")
    print(f"BUILD_MODE={BUILD_MODE}")
    print(f"BUILD_DIR={BUILD_DIR}")
    print(f"HIGH_PRECISION={HIGH_PRECISION}")
    print(f"PROFILE_MODE={PROFILE_MODE}")
    print(f"ANNOTATION_MODE={ANNOTATION_MODE}")
    print(f"PARALLEL_BUILD={PARALLEL_BUILD}")
    print(f"COPY_TO_SOURCE={COPY_TO_SOURCE}")
    print(f"FORCE_STRIP={FORCE_STRIP}")
    print(f"PYO3_ONLY={PYO3_ONLY}")
    print_env_var_if_exists("CC")
    print_env_var_if_exists("CXX")
    print_env_var_if_exists("LDSHARED")
    print_env_var_if_exists("CFLAGS")
    print_env_var_if_exists("LDFLAGS")
    print_env_var_if_exists("LD_LIBRARY_PATH")
    print_env_var_if_exists("PYO3_PYTHON")
    print_env_var_if_exists("PYTHONHOME")
    print_env_var_if_exists("RUSTFLAGS")
    print_env_var_if_exists("DRY_RUN")

    if DRY_RUN:
        show_rustanalyzer_settings()
    else:
        print("\nStarting build...")
        ts_start = dt.datetime.now(dt.UTC)
        build()
        print(f"Build time: {dt.datetime.now(dt.UTC) - ts_start}")
        print("\033[32m" + "Build completed" + "\033[0m")
