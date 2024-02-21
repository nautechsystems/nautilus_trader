# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

from typing import Any

import msgspec

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.common.config import resolve_config_path
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import ExecAlgorithmId


class ExecEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``ExecutionEngine`` instances.

    Parameters
    ----------
    load_cache : bool, default True
        If the cache should be loaded on initialization.
    allow_cash_positions : bool, default True
        If unleveraged spot/cash assets should generate positions.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    load_cache: bool = True
    allow_cash_positions: bool = True
    debug: bool = False


class ExecAlgorithmConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all execution algorithm configurations.

    Parameters
    ----------
    exec_algorithm_id : ExecAlgorithmId, optional
        The unique ID for the execution algorithm.
        If not ``None`` then will become the execution algorithm ID.

    """

    exec_algorithm_id: ExecAlgorithmId | None = None


class ImportableExecAlgorithmConfig(NautilusConfig, frozen=True):
    """
    Configuration for an execution algorithm instance.

    Parameters
    ----------
    exec_algorithm_path : str
        The fully qualified name of the execution algorithm class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The execution algorithm configuration.

    """

    exec_algorithm_path: str
    config_path: str
    config: dict[str, Any]


class ExecAlgorithmFactory:
    """
    Provides execution algorithm creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableExecAlgorithmConfig):
        """
        Create an execution algorithm from the given configuration.

        Parameters
        ----------
        config : ImportableExecAlgorithmConfig
            The configuration for the building step.

        Returns
        -------
        ExecAlgorithm

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableExecAlgorithmConfig`.

        """
        PyCondition.type(config, ImportableExecAlgorithmConfig, "config")
        exec_algorithm_cls = resolve_path(config.exec_algorithm_path)
        config_cls = resolve_config_path(config.config_path)
        json = msgspec.json.encode(config.config, enc_hook=msgspec_encoding_hook)
        config = config_cls.parse(json)
        return exec_algorithm_cls(config=config)
