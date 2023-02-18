#!/usr/bin/env python3

import itertools
import os
import platform
import shutil
import subprocess
import sysconfig
from datetime import datetime
from pathlib import Path

import numpy as np
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
PARALLEL_BUILD = True if os.getenv("PARALLEL_BUILD", "true") == "true" else False
# If COPY_TO_SOURCE is enabled, copy built *.so files back into the source tree
COPY_TO_SOURCE = True if os.getenv("COPY_TO_SOURCE", "true") == "true" else False
# If PyO3 only then don't build C extensions to reduce compilation time
PYO3_ONLY = False if os.getenv("PYO3_ONLY", "") == "" else True

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
if platform.system() != "Darwin":
    # Use clang as the default compiler
    os.environ["CC"] = "clang"
    os.environ["LDSHARED"] = "clang -shared"

TARGET_DIR = os.path.join(os.getcwd(), "nautilus_core", "target", BUILD_MODE)

if platform.system() == "Windows":
    # https://docs.microsoft.com/en-US/cpp/error-messages/tool-errors/linker-tools-error-lnk1181?view=msvc-170&viewFallbackFrom=vs-2019
    os.environ["LIBPATH"] = os.environ.get("LIBPATH", "") + f":{TARGET_DIR}"
    RUST_LIB_PFX = ""
    RUST_STATIC_LIB_EXT = "lib"
    RUST_DYLIB_EXT = "dll"
    TARGET_DIR = TARGET_DIR.replace(BUILD_MODE, "x86_64-pc-windows-msvc/" + BUILD_MODE)
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
RUST_LIBS = [
    f"{TARGET_DIR}/{RUST_LIB_PFX}nautilus_common.{RUST_STATIC_LIB_EXT}",
    f"{TARGET_DIR}/{RUST_LIB_PFX}nautilus_core.{RUST_STATIC_LIB_EXT}",
    f"{TARGET_DIR}/{RUST_LIB_PFX}nautilus_model.{RUST_STATIC_LIB_EXT}",
    f"{TARGET_DIR}/{RUST_LIB_PFX}nautilus_persistence.{RUST_STATIC_LIB_EXT}",
]


def _build_rust_libs() -> None:
    try:
        # Build the Rust libraries using Cargo
        build_options = ""
        extra_flags = ""
        if platform.system() == "Windows":
            extra_flags = " --target x86_64-pc-windows-msvc"

        build_options += " --release" if BUILD_MODE == "release" else ""
        print("Compiling Rust libraries...")
        build_cmd = f"(cd nautilus_core && cargo build{build_options}{extra_flags} --all-features)"
        print(build_cmd)
        os.system(build_cmd)  # noqa
    except subprocess.CalledProcessError as e:
        raise RuntimeError(
            f"Error running cargo: {e.stderr.decode()}",
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
    define_macros = [("NPY_NO_DEPRECATED_API", "NPY_1_7_API_VERSION")]
    if PROFILE_MODE or ANNOTATION_MODE:
        # Profiling requires special macro directives
        define_macros.append(("CYTHON_TRACE", "1"))

    extra_compile_args = []
    if platform.system() == "Darwin":
        extra_compile_args.append("-Wno-unreachable-code-fallthrough")

    if platform.system() != "Windows":
        # Suppress warnings produced by Cython boilerplate
        extra_compile_args.append("-Wno-parentheses-equality")
        if BUILD_MODE == "release":
            extra_compile_args.append("-O2")
            extra_compile_args.append("-pipe")

    extra_link_args = RUST_LIBS
    if platform.system() == "Windows":
        extra_link_args += [
            "WS2_32.Lib",
            "AdvAPI32.Lib",
            "UserEnv.Lib",
            "bcrypt.lib",
        ]

    print("Creating C extension modules...")
    print(f"define_macros={define_macros}")
    print(f"extra_compile_args={extra_compile_args}")

    return [
        Extension(
            name=str(pyx.relative_to(".")).replace(os.path.sep, ".")[:-4],
            sources=[str(pyx)],
            include_dirs=[np.get_include()] + RUST_INCLUDES,
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
        dict(
            name="nautilus_trader",
            ext_modules=cythonize(
                module_list=extensions,
                compiler_directives=CYTHON_COMPILER_DIRECTIVES,
                nthreads=nthreads,
                build_dir=BUILD_DIR,
                gdb_debug=PROFILE_MODE,
            ),
            zip_safe=False,
        ),
    )
    return distribution


def _copy_build_dir_to_project(cmd: build_ext) -> None:
    # Copy built extensions back to the project tree
    for output in cmd.get_outputs():
        relative_extension = os.path.relpath(output, cmd.build_lib)
        if not os.path.exists(output):
            continue

        # Copy the file and set permissions
        shutil.copyfile(output, relative_extension)
        mode = os.stat(relative_extension).st_mode
        mode |= (mode & 0o444) >> 2
        os.chmod(relative_extension, mode)

    print("Copied all compiled dynamic library files into source")


def _copy_rust_dylibs_to_project() -> None:
    # https://pyo3.rs/latest/building_and_distribution#manual-builds
    ext_suffix = sysconfig.get_config_var("EXT_SUFFIX")
    src = f"{TARGET_DIR}/{RUST_LIB_PFX}nautilus_pyo3.{RUST_DYLIB_EXT}"
    dst = f"nautilus_trader/core/nautilus_pyo3{ext_suffix}"
    shutil.copyfile(src=src, dst=dst)

    print(f"Copied {src} to {dst}")


def _get_clang_version() -> str:
    try:
        result = subprocess.run(
            "clang --version",
            check=True,
            shell=True,
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
    except subprocess.CalledProcessError as e:
        raise RuntimeError(
            "You are installing from source which requires the Clang compiler to be installed.\n"
            f"Error running clang: {e.stderr.decode()}",
        ) from e


def _get_rustc_version() -> str:
    try:
        result = subprocess.run(
            "rustc --version",
            check=True,
            shell=True,
            capture_output=True,
        )
        output = result.stdout.decode().lstrip("rustc ")[:-1]
        return output
    except subprocess.CalledProcessError as e:
        raise RuntimeError(
            "You are installing from source which requires the Rust compiler to "
            "be installed.\nFind more information at https://www.rust-lang.org/tools/install\n"
            f"Error running rustc: {e.stderr.decode()}",
        ) from e


def build(pyo3_only=False) -> None:
    """Construct the extensions and distribution."""  # noqa
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


if __name__ == "__main__":
    print("\033[36m")
    print("=====================================================================")
    print("Nautilus Builder")
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
    ts_start = datetime.utcnow()
    build()
    print(f"Build time: {datetime.utcnow() - ts_start}")
    print("\033[32m" + "Build completed" + "\033[0m")
