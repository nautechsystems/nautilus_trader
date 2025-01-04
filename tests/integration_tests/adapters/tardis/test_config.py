# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

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
