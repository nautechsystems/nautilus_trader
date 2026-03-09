from __future__ import annotations

from io import StringIO
import logging
import warnings

import pytest

from flux.runners.shared.logging import DEFAULT_LOG_FORMAT
from flux.runners.shared.logging import build_node_logging_config
from flux.runners.shared.logging import configure_service_logging
from flux.runners.shared.logging import emit_startup_banner
from flux.runners.shared.logging import resolve_runner_log_level


def teardown_function() -> None:
    logging.captureWarnings(False)
    root = logging.getLogger()
    for handler in list(root.handlers):
        root.removeHandler(handler)
        handler.close()
    root.setLevel(logging.NOTSET)


def test_resolve_runner_log_level_prefers_cli_then_service_env_then_global_env_then_config(
    monkeypatch,
) -> None:
    monkeypatch.setenv("FLUX_API_LOG_LEVEL", "WARNING")
    monkeypatch.setenv("FLUX_LOG_LEVEL", "ERROR")

    assert (
        resolve_runner_log_level(
            cli_level="DEBUG",
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )
        == "DEBUG"
    )
    assert (
        resolve_runner_log_level(
            cli_level=None,
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )
        == "WARNING"
    )

    monkeypatch.delenv("FLUX_API_LOG_LEVEL")
    assert (
        resolve_runner_log_level(
            cli_level=None,
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )
        == "ERROR"
    )

    monkeypatch.delenv("FLUX_LOG_LEVEL")
    assert (
        resolve_runner_log_level(
            cli_level=None,
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )
        == "INFO"
    )


def test_configure_service_logging_routes_info_to_stdout_and_warning_to_stderr(
    monkeypatch,
) -> None:
    stdout = StringIO()
    stderr = StringIO()
    monkeypatch.setattr("sys.stdout", stdout)
    monkeypatch.setattr("sys.stderr", stderr)

    logger = configure_service_logging(
        cli_level=None,
        config_level="INFO",
        service_env_var="FLUX_BRIDGE_LOG_LEVEL",
        logger_name="flux-test",
    )

    logger.info("hello")
    logger.warning("warn")

    for handler in logging.getLogger().handlers:
        handler.flush()

    stdout_value = stdout.getvalue()
    stderr_value = stderr.getvalue()

    assert "INFO flux-test - hello" in stdout_value
    assert "hello" not in stderr_value
    assert "warn" not in stdout_value
    assert "WARNING flux-test - warn" in stderr_value


def test_configure_service_logging_uses_stable_formatter(
    monkeypatch,
) -> None:
    monkeypatch.setattr("sys.stdout", StringIO())
    monkeypatch.setattr("sys.stderr", StringIO())

    configure_service_logging(
        cli_level=None,
        config_level="INFO",
        service_env_var="FLUX_PORTFOLIO_LOG_LEVEL",
        logger_name="flux-test",
    )

    assert logging.getLogger().handlers
    assert all(
        handler.formatter is not None and handler.formatter._fmt == DEFAULT_LOG_FORMAT
        for handler in logging.getLogger().handlers
    )


def test_resolve_runner_log_level_rejects_invalid_explicit_levels(monkeypatch) -> None:
    with pytest.raises(ValueError, match="cli"):
        resolve_runner_log_level(
            cli_level="LOUD",
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )

    monkeypatch.setenv("FLUX_API_LOG_LEVEL", "LOUD")
    with pytest.raises(ValueError, match="FLUX_API_LOG_LEVEL"):
        resolve_runner_log_level(
            cli_level=None,
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )

    monkeypatch.delenv("FLUX_API_LOG_LEVEL")
    monkeypatch.setenv("FLUX_LOG_LEVEL", "LOUD")
    with pytest.raises(ValueError, match="FLUX_LOG_LEVEL"):
        resolve_runner_log_level(
            cli_level=None,
            config_level="INFO",
            service_env_var="FLUX_API_LOG_LEVEL",
        )

    monkeypatch.delenv("FLUX_LOG_LEVEL")
    with pytest.raises(ValueError, match="config"):
        resolve_runner_log_level(
            cli_level=None,
            config_level="LOUD",
            service_env_var="FLUX_API_LOG_LEVEL",
        )


def test_configure_service_logging_leaves_warning_capture_unchanged(monkeypatch) -> None:
    monkeypatch.setattr("sys.stdout", StringIO())
    monkeypatch.setattr("sys.stderr", StringIO())
    original_showwarning = warnings.showwarning

    configure_service_logging(
        cli_level=None,
        config_level="INFO",
        service_env_var="FLUX_API_LOG_LEVEL",
        logger_name="flux-test",
    )

    assert warnings.showwarning is original_showwarning


def test_emit_startup_banner_prints_even_when_info_logs_are_disabled(monkeypatch) -> None:
    stdout = StringIO()
    stderr = StringIO()
    monkeypatch.setattr("sys.stdout", stdout)
    monkeypatch.setattr("sys.stderr", stderr)

    configure_service_logging(
        cli_level="WARNING",
        config_level="INFO",
        service_env_var="FLUX_API_LOG_LEVEL",
        logger_name="flux-test",
    )

    emit_startup_banner(prefix="tokenmm-run-api", message="profiles ready")

    assert stdout.getvalue().strip().endswith("[tokenmm-run-api] profiles ready")
    assert stderr.getvalue() == ""


def test_build_node_logging_config_keeps_nodes_journal_first(monkeypatch) -> None:
    monkeypatch.setenv("FLUX_NODE_LOG_LEVEL", "DEBUG")

    config = build_node_logging_config(
        cli_level=None,
        config_level="INFO",
    )

    assert config.log_level == "DEBUG"
    assert config.log_level_file is None
    assert config.log_directory is None
    assert config.use_pyo3 is True
