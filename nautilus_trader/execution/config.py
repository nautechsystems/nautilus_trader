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

from __future__ import annotations

from typing import Any

import msgspec

from nautilus_trader.common.config import NautilusConfig
from nautilus_trader.common.config import PositiveFloat
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.common.config import resolve_config_path
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ExecAlgorithmId


class ExecEngineConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``ExecutionEngine`` instances.

    Parameters
    ----------
    load_cache : bool, default True
        If the cache should be loaded on initialization.
    manage_own_order_books : bool, default False
        If the execution engine should maintain own/user order books based on commands and events.
    snapshot_orders : bool, default False
        If order state snapshot lists are persisted to a backing database.
        Snapshots will be taken at every order state update (when events are applied).
    snapshot_positions : bool, default False
        If position state snapshot lists are persisted to a backing database.
        Snapshots will be taken at position opened, changed and closed (when events are applied).
        To include the unrealized PnL in the snapshot then quotes for the positions instrument must
        be available in the cache.
    snapshot_positions_interval_secs : PositiveFloat, optional
        The interval (seconds) at which *additional* position state snapshots are persisted to a
        backing database.
        If ``None`` then no additional snapshots will be taken.
        To include unrealized PnL in these snapshots, quotes for the position's instrument must be
        available in the cache.
    convert_quote_qty_to_base : bool, default True
        If quote-denominated order quantities should be converted to base units before submission.
        Deprecated: future releases will remove this automatic conversion. Set ``False`` to keep
        behaviour consistent with venues which expect quote-denominated quantities.
    external_clients : list[ClientId], optional
        Client IDs representing external execution streams.
        Commands with these client IDs will be published on the message bus only;
        the execution engine will not attempt to forward them to a local `ExecutionClient`.
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    """

    load_cache: bool = True
    manage_own_order_books: bool = False
    convert_quote_qty_to_base: bool = True
    snapshot_orders: bool = False
    snapshot_positions: bool = False
    snapshot_positions_interval_secs: PositiveFloat | None = None
    external_clients: list[ClientId] | None = None
    debug: bool = False


class ExecAlgorithmConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all execution algorithm configurations.

    Parameters
    ----------
    exec_algorithm_id : ExecAlgorithmId, optional
        The unique ID for the execution algorithm.
        If not ``None`` then will become the execution algorithm ID.
    log_events : bool, default True
        If events should be logged by the execution algorithm.
        If False, then only warning events and above are logged.
    log_commands : bool, default True
        If commands should be logged by the execution algorithm.

    """

    exec_algorithm_id: ExecAlgorithmId | None = None
    log_events: bool = True
    log_commands: bool = True


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
