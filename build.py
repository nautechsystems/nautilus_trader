#!/usr/bin/env python3

import datetime
import itertools
import os
import platform
import shutil
import subprocess
import sysconfig
from pathlib import Path

import numpy as np
import toml
from Cython.Build import build_ext
from Cython.Build import cythonize
from Cython.Compiler import Options
from Cython.Compiler.Version import version as cython_compiler_version
from setuptools import Distribution
from setuptools import Extension


# The build mode (affects cargo)
BUILD_MODE = os.getenv("BUILD_MODE", "release")
# If PROFILE_MODE mode is enabled, include traces necessary for coverage and profiling
PROFILE_MODE = bool(os.getenv("PROFILE_MODE", ""))
# If ANNOTATION mode is enabled, generate an annotated HTML version of the input source files
ANNOTATION_MODE = bool(os.getenv("ANNOTATION_MODE", ""))
# If PARALLEL build is enabled, uses all CPUs for compile stage of build
PARALLEL_BUILD = os.getenv("PARALLEL_BUILD", "true") == "true"
# If COPY_TO_SOURCE is enabled, copy built *.so files back into the source tree
COPY_TO_SOURCE = os.getenv("COPY_TO_SOURCE", "true") == "true"
# If PyO3 only then don't build C extensions to reduce compilation time
PYO3_ONLY = os.getenv("PYO3_ONLY", "") != ""

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
if platform.system() == "Linux":
    # Use clang as the default compiler
    os.environ["CC"] = "clang"
    os.environ["LDSHARED"] = "clang -shared"

TARGET_DIR = Path.cwd() / "nautilus_core" / "target" / BUILD_MODE

if platform.system() == "Windows":
    # Linker error 1181
    # https://docs.microsoft.com/en-US/cpp/error-messages/tool-errors/linker-tools-error-lnk1181?view=msvc-170&viewFallbackFrom=vs-2019
    RUST_LIB_PFX = ""
    RUST_STATIC_LIB_EXT = "lib"
    RUST_DYLIB_EXT = "dll"
elif platform.system() == "Darwin":
    RUST_LIB_PFX = "lib"
    RUST_STATIC_LIB_EXT = "a"
    RUST_DYLIB_EXT = "dylib"
else:  # Linux
    RUST_LIB_PFX = "lib"
    RUST_STATIC_LIB_EXT = "a"
    RUST_DYLIB_EXT = "so"

# Directories with headers to include
RUST_INCLUDES = ["nautilus_trader/core/includes"]
RUST_LIB_PATHS: list[Path] = [
    TARGET_DIR / f"{RUST_LIB_PFX}nautilus_backtest.{RUST_STATIC_LIB_EXT}",
    TARGET_DIR / f"{RUST_LIB_PFX}nautilus_common.{RUST_STATIC_LIB_EXT}",
    TARGET_DIR / f"{RUST_LIB_PFX}nautilus_core.{RUST_STATIC_LIB_EXT}",
    TARGET_DIR / f"{RUST_LIB_PFX}nautilus_model.{RUST_STATIC_LIB_EXT}",
    TARGET_DIR / f"{RUST_LIB_PFX}nautilus_persistence.{RUST_STATIC_LIB_EXT}",
]
RUST_LIBS: list[str] = [str(path) for path in RUST_LIB_PATHS]


def _build_rust_libs() -> None:
    try:
        # Build the Rust libraries using Cargo
        build_options = " --release" if BUILD_MODE == "release" else ""
        print("Compiling Rust libraries...")

        cmd_args = [
            "cargo",
            "build",
            *build_options.split(),
            "--all-features",
        ]
        print(" ".join(cmd_args))

        subprocess.run(
            cmd_args,  # noqa
            cwd="nautilus_core",
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
Options.fast_fail = True  # Abort compilation on first error
Options.warning_errors = True  # Treat compiler warnings as errors
Options.extra_warnings = True

CYTHON_COMPILER_DIRECTIVES = {
    "language_level": "3",
    "cdivision": True,  # If division is as per C with no check for zero (35% speed up)
    "nonecheck": True,  # Insert extra check for field access on C extensions
    "embedsignature": True,  # If docstrings should be embedded into C signatures
    "profile": PROFILE_MODE,  # If we're debugging or profiling
    "linetrace": PROFILE_MODE,  # If we're debugging or profiling
    "warn.maybe_uninitialized": True,
}


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

    if platform.system() != "Windows":
        # Suppress warnings produced by Cython boilerplate
        extra_compile_args.append("-Wno-unreachable-code")
        if BUILD_MODE == "release":
            extra_compile_args.append("-O2")
            extra_compile_args.append("-pipe")

    if platform.system() == "Windows":
        extra_link_args += [
            "AdvAPI32.Lib",
            "bcrypt.lib",
            "Crypt32.lib",
            "Iphlpapi.lib",
            "Kernel32.lib",
            "ncrypt.lib",
            "Netapi32.lib",
            "ntdll.lib",
            "Ole32.lib",
            "OleAut32.lib",
            "Pdh.lib",
            "PowrProf.lib",
            "Psapi.lib",
            "schannel.lib",
            "secur32.lib",
            "Shell32.lib",
            "User32.Lib",
            "UserEnv.Lib",
            "WS2_32.Lib",
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
    if platform.system() == "Windows":
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
    # https://pyo3.rs/latest/building_and_distribution#manual-builds
    ext_suffix = sysconfig.get_config_var("EXT_SUFFIX")
    src = Path(TARGET_DIR) / f"{RUST_LIB_PFX}nautilus_pyo3.{RUST_DYLIB_EXT}"
    dst = Path("nautilus_trader/core") / f"nautilus_pyo3{ext_suffix}"
    shutil.copyfile(src=src, dst=dst)

    print(f"Copied {src} to {dst}")


def _get_clang_version() -> str:
    try:
        result = subprocess.run(
            ["clang", "--version"],  # noqa
            check=True,
            capture_output=True,
        )
        output = (
            result.stdout.decode()
            .splitlines()[0]
            .lstrip("Apple ")
            .lstrip("Ubuntu ")
            .lstrip("clang version ")
        )
        return output
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        err_msg = str(e) if isinstance(e, FileNotFoundError) else e.stderr.decode()
        raise RuntimeError(
            "You are installing from source which requires the Clang compiler to be installed.\n"
            f"Error running clang: {err_msg}",
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


def _strip_unneeded_symbols() -> None:
    try:
        print("Stripping unneeded symbols from binaries...")
        for so in itertools.chain(Path("nautilus_trader").rglob("*.so")):
            if platform.system() == "Linux":
                strip_cmd = ["strip", "--strip-unneeded", so]
            elif platform.system() == "Darwin":
                strip_cmd = ["strip", "-x", so]
            else:
                raise RuntimeError(f"Cannot strip symbols for platform {platform.system()}")
            subprocess.run(
                strip_cmd,  # type: ignore [arg-type] # noqa
                check=True,
                capture_output=True,
            )
    except subprocess.CalledProcessError as e:
        raise RuntimeError(f"Error when stripping symbols.\n{e}") from e


def build() -> None:
    """
    Construct the extensions and distribution.
    """
    _build_rust_libs()
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

    if BUILD_MODE == "release" and platform.system() in ("Linux", "Darwin"):
        # Only strip symbols for release builds
        _strip_unneeded_symbols()


if __name__ == "__main__":
    nautilus_trader_version = toml.load("pyproject.toml")["tool"]["poetry"]["version"]
    print("\033[36m")
    print("=====================================================================")
    print(f"Nautilus Builder {nautilus_trader_version}")
    print("=====================================================================\033[0m")
    print(f"System: {platform.system()} {platform.machine()}")
    print(f"Clang:  {_get_clang_version()}")
    print(f"Rust:   {_get_rustc_version()}")
    print(f"Python: {platform.python_version()}")
    print(f"Cython: {cython_compiler_version}")
    print(f"NumPy:  {np.__version__}\n")

    print(f"BUILD_MODE={BUILD_MODE}")
    print(f"BUILD_DIR={BUILD_DIR}")
    print(f"PROFILE_MODE={PROFILE_MODE}")
    print(f"ANNOTATION_MODE={ANNOTATION_MODE}")
    print(f"PARALLEL_BUILD={PARALLEL_BUILD}")
    print(f"COPY_TO_SOURCE={COPY_TO_SOURCE}")
    print(f"PYO3_ONLY={PYO3_ONLY}\n")

    print("Starting build...")
    ts_start = datetime.datetime.now(datetime.timezone.utc)
    build()
    print(f"Build time: {datetime.datetime.now(datetime.timezone.utc) - ts_start}")
    print("\033[32m" + "Build completed" + "\033[0m")
