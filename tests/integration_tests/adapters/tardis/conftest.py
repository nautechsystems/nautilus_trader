from pathlib import Path

import pytest

from nautilus_trader import PACKAGE_ROOT


def get_test_data_path(file_name: str) -> Path:
    """
    Get path to test data file in the tardis test data directory.
    """
    path = PACKAGE_ROOT / "crates" / "adapters" / "tardis" / "test_data" / "csv" / file_name
    assert path.exists(), f"Test data file not found: {path}"
    return path


@pytest.fixture
def instrument_provider():
    pass  # Not applicable


@pytest.fixture
def data_client():
    pass  # Not applicable


@pytest.fixture
def exec_client():
    pass  # Not applicable


@pytest.fixture
def instrument():
    pass  # Not applicable


@pytest.fixture
def account_state():
    pass  # Not applicable
