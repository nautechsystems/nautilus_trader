import tempfile

import nox
from nox.sessions import Session


ALL_EXTRAS = "ib redis"


# Ensure everything runs within Poetry venvs
nox.options.error_on_external_run = True


@nox.session
def tests(session: Session) -> None:
    """Run the test suite."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    _run_pytest(
        session,
        "--ignore=tests/integration_tests/",
        "--ignore=tests/performance_tests/",
    )


@nox.session
def tests_all(session: Session) -> None:
    """Run the test suite including integration tests."""
    _setup_poetry(session, "--extras", ALL_EXTRAS, env={"PYTHONDEVMODE": "1"})
    _run_pytest(session)


@nox.session
def tests_integration(session: Session) -> None:
    """Run the integration test suite."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    _run_pytest(session, "tests/integration_tests/")


@nox.session
def tests_performance(session: Session) -> None:
    """Run the performance test suite."""
    _setup_poetry(session, "--extras", ALL_EXTRAS)
    _run_pytest(
        session,
        "tests/performance_tests/",
        "--benchmark-json=output.json",
        parallel=False,
    )


@nox.session
def coverage(session: Session) -> None:
    """Run with test coverage."""
    _setup_poetry(session, "--extras", ALL_EXTRAS, env={"PROFILE_MODE": "true"})
    _run_coverage(session)


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

    env = kwargs.get("env", {})

    if "no-parallel" in session.posargs:
        # Ensure deterministic builds by disabling parallelism
        env["PARALLEL_BUILD"] = ""  # Empty string parsed as false

    kwargs["env"] = env

    # Install poetry, then install the project (with its dependencies)
    session.install("poetry")
    session.run("poetry", "install", *args, **kwargs)


def _run_pytest(session: Session, *args, parallel: bool = False) -> None:
    pytest_args = [
        "poetry",
        "run",
        "pytest",
        *args,
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
        # There is an odd Coverage/Cython bug when using pytest-xdist,
        # so we have to run tests single-threaded here.
        parallel=False,
    )
