from __future__ import annotations

import logging
import os
import sys
from typing import Any

from nautilus_trader.config import LoggingConfig


DEFAULT_LOG_FORMAT = "%(asctime)s %(levelname)s %(name)s - %(message)s"
DEFAULT_LOG_LEVEL = "INFO"
GLOBAL_LOG_LEVEL_ENV_VAR = "FLUX_LOG_LEVEL"
NODE_LOG_LEVEL_ENV_VAR = "FLUX_NODE_LOG_LEVEL"
BRIDGE_LOG_LEVEL_ENV_VAR = "FLUX_BRIDGE_LOG_LEVEL"
PORTFOLIO_LOG_LEVEL_ENV_VAR = "FLUX_PORTFOLIO_LOG_LEVEL"
API_LOG_LEVEL_ENV_VAR = "FLUX_API_LOG_LEVEL"

_LEVEL_ALIASES = {
    "FATAL": "CRITICAL",
    "WARN": "WARNING",
}


class _MaxLevelFilter(logging.Filter):
    def __init__(self, max_level: int) -> None:
        super().__init__()
        self._max_level = max_level

    def filter(self, record: logging.LogRecord) -> bool:
        return record.levelno <= self._max_level


def _normalize_level_name(value: Any) -> str | None:
    if value is None:
        return None

    raw_text = str(value).strip()
    if not raw_text:
        return None

    text = raw_text.upper()
    text = _LEVEL_ALIASES.get(text, text)
    if text in logging.getLevelNamesMapping():
        return text

    return None


def _resolve_explicit_level_name(value: Any, *, source_name: str) -> str | None:
    normalized = _normalize_level_name(value)
    if normalized is not None:
        return normalized

    if value is None or not str(value).strip():
        return None

    raise ValueError(f"Invalid log level {value!r} from {source_name}")


def resolve_runner_log_level(
    *,
    cli_level: str | None,
    config_level: Any,
    service_env_var: str,
) -> str:
    for source_name, candidate in (
        ("cli", cli_level),
        (service_env_var, os.getenv(service_env_var)),
        (GLOBAL_LOG_LEVEL_ENV_VAR, os.getenv(GLOBAL_LOG_LEVEL_ENV_VAR)),
        ("config", config_level),
        ("default", DEFAULT_LOG_LEVEL),
    ):
        normalized = _resolve_explicit_level_name(candidate, source_name=source_name)
        if normalized:
            return normalized

    return DEFAULT_LOG_LEVEL


def _reset_root_logger(root: logging.Logger) -> None:
    for handler in list(root.handlers):
        root.removeHandler(handler)
        handler.close()


def configure_python_logging(
    *,
    cli_level: str | None,
    config_level: Any,
    service_env_var: str,
    logger_name: str | None = None,
) -> logging.Logger:
    level_name = resolve_runner_log_level(
        cli_level=cli_level,
        config_level=config_level,
        service_env_var=service_env_var,
    )
    level_number = logging.getLevelNamesMapping()[level_name]
    formatter = logging.Formatter(DEFAULT_LOG_FORMAT)

    root = logging.getLogger()
    _reset_root_logger(root)
    root.setLevel(level_number)

    stdout_handler = logging.StreamHandler(sys.stdout)
    stdout_handler.setLevel(logging.DEBUG)
    stdout_handler.addFilter(_MaxLevelFilter(logging.INFO))
    stdout_handler.setFormatter(formatter)

    stderr_handler = logging.StreamHandler(sys.stderr)
    stderr_handler.setLevel(logging.WARNING)
    stderr_handler.setFormatter(formatter)

    root.addHandler(stdout_handler)
    root.addHandler(stderr_handler)

    return logging.getLogger(logger_name) if logger_name else logging.getLogger()


def emit_startup_banner(*, prefix: str, message: str) -> None:
    print(f"[{prefix}] {message}", file=sys.stdout, flush=True)


def configure_service_logging(
    *,
    cli_level: str | None,
    config_level: Any,
    service_env_var: str,
    logger_name: str,
) -> logging.Logger:
    return configure_python_logging(
        cli_level=cli_level,
        config_level=config_level,
        service_env_var=service_env_var,
        logger_name=logger_name,
    )


def build_node_logging_config(
    *,
    cli_level: str | None = None,
    config_level: Any,
) -> LoggingConfig:
    return LoggingConfig(
        log_level=resolve_runner_log_level(
            cli_level=cli_level,
            config_level=config_level,
            service_env_var=NODE_LOG_LEVEL_ENV_VAR,
        ),
        use_pyo3=True,
    )


__all__ = [
    "API_LOG_LEVEL_ENV_VAR",
    "BRIDGE_LOG_LEVEL_ENV_VAR",
    "DEFAULT_LOG_FORMAT",
    "GLOBAL_LOG_LEVEL_ENV_VAR",
    "NODE_LOG_LEVEL_ENV_VAR",
    "PORTFOLIO_LOG_LEVEL_ENV_VAR",
    "build_node_logging_config",
    "emit_startup_banner",
    "configure_python_logging",
    "configure_service_logging",
    "resolve_runner_log_level",
]
