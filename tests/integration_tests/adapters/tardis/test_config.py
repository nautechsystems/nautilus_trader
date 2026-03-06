import pkgutil

from nautilus_trader.core import nautilus_pyo3


def test_tardis_config_replay_options():
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.tardis.resources",
        "replay_options.json",
    )
    assert data

    # Act
    options = nautilus_pyo3.ReplayNormalizedRequestOptions.from_json(data)

    # Assert
    assert isinstance(options, nautilus_pyo3.ReplayNormalizedRequestOptions)


def test_tardis_config_replay_options_array():
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.tardis.resources",
        "replay_options_array.json",
    )
    assert data

    # Act
    options = nautilus_pyo3.ReplayNormalizedRequestOptions.from_json_array(data)

    # Assert
    assert isinstance(options[0], nautilus_pyo3.ReplayNormalizedRequestOptions)


def test_tardis_config_stream_options():
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.tardis.resources",
        "stream_options.json",
    )
    assert data

    # Act
    options = nautilus_pyo3.StreamNormalizedRequestOptions.from_json(data)

    # Assert
    assert isinstance(options, nautilus_pyo3.StreamNormalizedRequestOptions)


def test_tardis_config_stream_options_array():
    # Arrange
    data = pkgutil.get_data(
        "tests.integration_tests.adapters.tardis.resources",
        "stream_options_array.json",
    )
    assert data

    # Act
    options = nautilus_pyo3.StreamNormalizedRequestOptions.from_json_array(data)

    # Assert
    assert isinstance(options[0], nautilus_pyo3.StreamNormalizedRequestOptions)
