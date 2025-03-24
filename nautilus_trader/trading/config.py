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
from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.common.config import resolve_config_path
from nautilus_trader.common.config import resolve_path
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import StrategyId


class StrategyConfig(NautilusConfig, kw_only=True, frozen=True):
    """
    The base model for all trading strategy configurations.

    Parameters
    ----------
    strategy_id : StrategyId, optional
        The unique ID for the strategy. Will become the strategy ID if not None.
    order_id_tag : str, optional
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    use_uuid_client_order_ids : bool, default False
        If UUID4's should be used for client order ID values.
    oms_type : OmsType, optional
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs.
    external_order_claims : list[InstrumentId], optional
        The external order claim instrument IDs.
        External orders for matching instrument IDs will be associated with (claimed by) the strategy.
    manage_contingent_orders : bool, default False
        If OUO and OCO **open** contingent orders should be managed automatically by the strategy.
        Any emulated orders which are active local will be managed by the `OrderEmulator` instead.
    manage_gtd_expiry : bool, default False
        If all order GTD time in force expirations should be managed by the strategy.
        If True, then will ensure open orders have their GTD timers re-activated on start.
    log_events : bool, default True
        If events should be logged by the strategy.
        If False, then only warning events and above are logged.
    log_commands : bool, default True
        If commands should be logged by the strategy.

    """

    strategy_id: StrategyId | None = None
    order_id_tag: str | None = None
    use_uuid_client_order_ids: bool = False
    oms_type: str | None = None
    external_order_claims: list[InstrumentId] | None = None
    manage_contingent_orders: bool = False
    manage_gtd_expiry: bool = False
    log_events: bool = True
    log_commands: bool = True


class ImportableStrategyConfig(NautilusConfig, frozen=True):
    """
    Configuration for a trading strategy instance.

    Parameters
    ----------
    strategy_path : str
        The fully qualified name of the strategy class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The strategy configuration.

    """

    strategy_path: str
    config_path: str
    config: dict[str, Any]


class StrategyFactory:
    """
    Provides strategy creation from importable configurations.
    """

    @staticmethod
    def create(config: ImportableStrategyConfig):
        """
        Create a trading strategy from the given configuration.

        Parameters
        ----------
        config : ImportableStrategyConfig
            The configuration for the building step.

        Returns
        -------
        Strategy

        Raises
        ------
        TypeError
            If `config` is not of type `ImportableStrategyConfig`.

        """
        PyCondition.type(config, ImportableStrategyConfig, "config")
        strategy_cls = resolve_path(config.strategy_path)
        config_cls = resolve_config_path(config.config_path)
        json = msgspec.json.encode(config.config, enc_hook=msgspec_encoding_hook)
        config = config_cls.parse(json)
        return strategy_cls(config=config)


class ImportableControllerConfig(NautilusConfig, frozen=True):
    """
    Configuration for a controller instance.

    Parameters
    ----------
    controller_path : str
        The fully qualified name of the controller class.
    config_path : str
        The fully qualified name of the config class.
    config : dict[str, Any]
        The controller configuration.

    """

    controller_path: str
    config_path: str
    config: dict
