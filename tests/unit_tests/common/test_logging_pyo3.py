import pytest

from nautilus_trader.common.component import is_logging_pyo3
from nautilus_trader.common.component import set_logging_pyo3
from nautilus_trader.core import nautilus_pyo3


@pytest.mark.parametrize(
    "invalid_level",
    [
        "INVALID",
        "DEBG",
        "WARNINGG",
        "FOO",
        "",
    ],
)
def test_init_logging_invalid_component_level_raises(invalid_level):
    with pytest.raises(Exception, match="Invalid log level string"):
        nautilus_pyo3.init_logging(
            trader_id=nautilus_pyo3.TraderId("TESTER-001"),
            instance_id=nautilus_pyo3.UUID4(),
            level_stdout=nautilus_pyo3.LogLevel.INFO,
            component_levels={"MyStrategy": invalid_level},
        )


def test_set_logging_pyo3_flag():
    initial = is_logging_pyo3()

    set_logging_pyo3(True)
    after_set = is_logging_pyo3()
    set_logging_pyo3(False)
    after_reset = is_logging_pyo3()

    assert after_set is True
    assert after_reset is False

    set_logging_pyo3(initial)


def test_logging_pyo3_flag_can_toggle_between_modes():
    set_logging_pyo3(True)
    assert is_logging_pyo3() is True

    set_logging_pyo3(False)
    assert is_logging_pyo3() is False

    set_logging_pyo3(True)
    assert is_logging_pyo3() is True

    set_logging_pyo3(False)
