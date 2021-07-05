#!/usr/bin/env python3

import itertools
import os
from pathlib import Path
import platform
import shutil
import sys
from typing import List

from Cython.Build import build_ext
from Cython.Build import cythonize
from Cython.Compiler import Options
from Cython.Compiler.Version import version as cython_compiler_version
import numpy as np
from setuptools import Distribution
from setuptools import Extension


# If DEBUG mode is enabled, skip compiler optimizations (TODO: implement)
DEBUG_MODE = bool(os.getenv("DEBUG_MODE", ""))
# If PROFILING mode is enabled, include traces necessary for coverage and profiling
PROFILING_MODE = bool(os.getenv("PROFILING_MODE", ""))
# If ANNOTATION mode is enabled, generate an annotated HTML version of the input source files
ANNOTATION_MODE = bool(os.getenv("ANNOTATION_MODE", ""))
# If PARALLEL build is enabled, uses all CPUs for compile stage of build
PARALLEL_BUILD = True if os.getenv("PARALLEL_BUILD", "true") == "true" else False
# If SKIP_BUILD_COPY is enabled, prevents copying built *.so files back into the source tree
SKIP_BUILD_COPY = bool(os.getenv("SKIP_BUILD_COPY", ""))

print(
    f"DEBUG_MODE={DEBUG_MODE}\n"
    f"PROFILING_MODE={PROFILING_MODE}\n"
    f"ANNOTATION_MODE={ANNOTATION_MODE}\n"
    f"PARALLEL_BUILD={PARALLEL_BUILD}\n"
    f"SKIP_BUILD_COPY={SKIP_BUILD_COPY}"
)

##########################
#  Cython build options  #
##########################
# https://cython.readthedocs.io/en/latest/src/userguide/source_files_and_compilation.html

Options.docstrings = True  # Include docstrings in modules
Options.fast_fail = True  # Abort the compilation on the first error occurred
Options.emit_code_comments = True
Options.annotate = ANNOTATION_MODE  # Create annotated HTML files for each .pyx
if ANNOTATION_MODE:
    Options.annotate_coverage_xml = "coverage.xml"
Options.fast_fail = True  # Abort compilation on first error
Options.warning_errors = True  # Treat compiler warnings as errors
Options.extra_warnings = True

CYTHON_COMPILER_DIRECTIVES = {
    "language_level": "3",
    "cdivision": True,  # If division is as per C with no check for zero (35% speed up)
    "embedsignature": True,  # If docstrings should be embedded into C signatures
    "profile": PROFILING_MODE,  # If we're profiling, turn on line tracing
    "linetrace": PROFILING_MODE,
    "warn.maybe_uninitialized": True,
    # "warn.unused_result": True,  # TODO(cs): Picks up legitimate unused variables
    # "warn.unused": True,  # TODO(cs): Fails on unused entry 'genexpr'
}


def _build_extensions() -> List[Extension]:
    # Regarding the compiler warning: #warning "Using deprecated NumPy API,
    # disable it with " "#define NPY_NO_DEPRECATED_API NPY_1_7_API_VERSION"
    # https://stackoverflow.com/questions/52749662/using-deprecated-numpy-api
    # From the Cython docs: "For the time being, it is just a warning that you can ignore."
    define_macros = [("NPY_NO_DEPRECATED_API", "NPY_1_7_API_VERSION")]
    if PROFILING_MODE or ANNOTATION_MODE:
        # Profiling requires special macro directives
        define_macros.append(("CYTHON_TRACE", "1"))

    extra_compile_args = []
    if platform.system() != "Windows":
        extra_compile_args.append("-O3")
        extra_compile_args.append("-pipe")

    print(f"define_macros={define_macros}")
    print(f"extra_compile_args={extra_compile_args}")

    return [
        Extension(
            name=str(pyx.relative_to(".")).replace(os.path.sep, ".")[:-4],
            sources=[str(pyx)],
            include_dirs=[".", np.get_include()],
            define_macros=define_macros,
            language="c",
            extra_compile_args=extra_compile_args,
        )
        for pyx in itertools.chain(
            Path("examples").rglob("*.pyx"),
            Path("nautilus_trader").rglob("*.pyx"),
        )
    ]


def _build_distribution(extensions: List[Extension]) -> Distribution:
    # Build a Distribution using cythonize()
    # Determine the build output directory
    if PROFILING_MODE:
        # For subsequent annotation, the C source needs to be in
        # the same tree as the Cython code.
        build_dir = None
    elif ANNOTATION_MODE:
        build_dir = "build/annotated"
    else:
        build_dir = "build/optimized"

    distribution = Distribution(
        dict(
            name="nautilus_trader",
            ext_modules=cythonize(
                module_list=extensions,
                compiler_directives=CYTHON_COMPILER_DIRECTIVES,
                nthreads=os.cpu_count(),
                build_dir=build_dir,
            ),
            zip_safe=False,
        )
    )
    distribution.package_dir = "nautilus_trader"
    return distribution


def _copy_build_dir_to_project(cmd: build_ext) -> None:
    # Copy built extensions back to the project tree
    for output in cmd.get_outputs():
        relative_extension = os.path.relpath(output, cmd.build_lib)
        if not os.path.exists(output):
            continue

        # Copy the file and set permissions
        print(f"Copying: {output} -> {relative_extension}")
        shutil.copyfile(output, relative_extension)
        mode = os.stat(relative_extension).st_mode
        mode |= (mode & 0o444) >> 2
        os.chmod(relative_extension, mode)


def build(setup_kwargs):
    """Construct the extensions and distribution."""  # noqa
    # Build C Extensions to feed into cythonize()
    extensions = _build_extensions()
    distribution = _build_distribution(extensions)

    # Build and run the command
    cmd: build_ext = build_ext(distribution)
    if PARALLEL_BUILD:
        cmd.parallel = os.cpu_count()
    cmd.ensure_finalized()
    cmd.run()

    # Copy the build back into the project for packaging
    _copy_build_dir_to_project(cmd)

    return setup_kwargs


if __name__ == "__main__":
    print("")

    # Work around a Cython problem in Python 3.8.x on macOS
    # https://github.com/cython/cython/issues/3262
    if platform.system() == "Darwin":
        print("macOS: Setting multiprocessing method to 'fork'.")
        try:
            # noinspection PyUnresolvedReferences
            import multiprocessing

            multiprocessing.set_start_method("fork", force=True)
        except ImportError:
            print("multiprocessing not available")

    print("Starting build...")
    # Note: On macOS (and perhaps other platforms), executable files may be
    # universal files containing multiple architectures. To determine the
    # “64-bitness” of the current interpreter, it is more reliable to query the
    # sys.maxsize attribute:
    bits = "64-bit" if sys.maxsize > 2 ** 32 else "32-bit"
    print(f"System: {platform.system()} {bits}")
    print(f"Python: {platform.python_version()}")
    print(f"Cython: {cython_compiler_version}")
    print(f"NumPy:  {np.__version__}")

    build({})
