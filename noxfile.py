import tempfile

import nox
from nox.sessions import Session


ALL_EXTRAS = "betfair ccxt docs ib oanda"


# Ensure everything runs within Poetry venvs
nox.options.error_on_external_run = True


@nox.session
def tests(session: Session) -> None:
    """Run the test suite."""
    _setup_poetry(session)
    _run_pytest(
        session,
        "--ignore=tests/integration_tests/",
        "--ignore=tests/performance_tests/",
    )


@nox.session
def tests_with_integration(session: Session) -> None:
    """Run the test suite including integration tests."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    _run_pytest(session, "--ignore=tests/performance_tests/")


@nox.session
def integration_tests(session: Session) -> None:
    """Run the integration test suite."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    _run_pytest(session, "tests/integration_tests/")


@nox.session
def performance_tests(session: Session) -> None:
    """Run the performance test suite."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    _run_pytest(
        session,
        "tests/performance_tests/",
        "--benchmark-json=PERF.JSON",
        parallel=False,
    )


@nox.session
def coverage(session: Session) -> None:
    """Run with test coverage."""
    _setup_poetry(session, "--extras", ALL_EXTRAS, env={"PROFILING_MODE": "true"})
    _run_coverage(session)


@nox.session
def build_docs(session: Session) -> None:
    """Build documentation."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    session.run("poetry", "run", "sphinx-build", "docs/source", "docs/build")


@nox.session
def safety(session):
    with tempfile.NamedTemporaryFile() as requirements:
        session.run(
            "poetry",
            "export",
            "--dev",
            "--format=requirements.txt",
            "--without-hashes",
            f"--output={requirements.name}",
            external=True,
        )
        session.install("safety")
        session.run("safety", "check", f"--file={requirements.name}", "--full-report")


def _setup_poetry(session: Session, *args, **kwargs) -> None:
    """Ensure that our environment is peaceful before running the session."""
    # Makes sure that poetry and our build requirements are installed.
    # Once they are, the package dependencies can be installed and the
    # actual package can be compiled.

    # No need to copy built *.so files back into the source tree
    env = kwargs.get("env", {})
    # Skip the build copy when using Nox.
    env["SKIP_BUILD_COPY"] = "true"
    kwargs["env"] = env

    # Install poetry, then install the project (with its dependencies)
    session.install("poetry")
    session.run("poetry", "install", *args, **kwargs)


def _run_pytest(session: Session, *args, parallel: bool = True) -> None:
    pytest_args = [
        "poetry",
        "run",
        "pytest",
        *args,
        *session.posargs,
        "--new-first",
        "--failed-first",
    ]
    if parallel:
        pytest_args += ["--numprocesses=auto", "--dist=loadscope"]

    session.run(*pytest_args)


def _run_coverage(session: Session):
    _run_pytest(
        session,
        "--ignore=tests/performance_tests/",
        "--cov-report=term",
        "--cov-report=xml",
        "--cov=nautilus_trader",
        # There is an odd Coverage/Cython bug when using pytest-xdist
        # so we have to run tests single-threaded here.
        parallel=False,
    )
