# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.flux.common.config import FLUX_DEFAULT_NAMESPACE
from nautilus_trader.flux.common.config import FLUX_SCHEMA_VERSION
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import validate_identifier_part
from nautilus_trader.flux.common.config import validate_schema_version
from nautilus_trader.flux.common.config import validate_symbol_part


class FluxRedisKeys:
    """
    Builder for Flux Redis keys under the versioned namespace.

    Notes
    -----
    By current protocol definition, params hash key and strategy-scoped params
    pubsub channel intentionally share the same Redis address.

    """

    def __init__(
        self,
        strategy_id: str,
        *,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> None:
        self._strategy_id = validate_identifier_part(strategy_id, "strategy_id")
        self._namespace = validate_identifier_part(namespace, "namespace")
        self._schema_version = validate_schema_version(schema_version, "schema_version")

    @classmethod
    def from_identity(cls, identity: FluxIdentityConfig) -> FluxRedisKeys:
        return cls(
            strategy_id=identity.strategy_id,
            namespace=identity.namespace,
            schema_version=identity.schema_version,
        )

    @property
    def strategy_id(self) -> str:
        return self._strategy_id

    @property
    def prefix(self) -> str:
        return f"{self._namespace}:{self._schema_version}"

    def state(self) -> str:
        return f"{self.prefix}:state:{self._strategy_id}"

    def events(self) -> str:
        return f"{self.prefix}:events:{self._strategy_id}"

    def trades_stream(self) -> str:
        return f"{self.prefix}:trades:stream:{self._strategy_id}"

    def alerts(self) -> str:
        return f"{self.prefix}:alerts:{self._strategy_id}"

    def fv_stream(self) -> str:
        return f"{self.prefix}:fv:stream:{self._strategy_id}"

    def balances_snapshot(self) -> str:
        return f"{self.prefix}:balances:snapshot:{self._strategy_id}"

    def balances_rows(self) -> str:
        return f"{self.prefix}:balances:rows:{self._strategy_id}"

    def market_last(self, exchange: str, base: str, quote: str) -> str:
        safe_exchange = validate_identifier_part(exchange, "exchange").lower()
        safe_base = validate_symbol_part(base, "base").upper()
        safe_quote = validate_symbol_part(quote, "quote").upper()
        return f"{self.prefix}:market:last:{self._strategy_id}:{safe_exchange}:{safe_base}_{safe_quote}"

    def params_hash_key(self) -> str:
        return f"{self.prefix}:params:{self._strategy_id}"

    def params_pubsub_channel(self) -> str:
        return f"{self.prefix}:params:{self._strategy_id}"

    # Backward-compatible aliases for initial Task 1 API names.
    def params_hash(self) -> str:
        return self.params_hash_key()

    def params_channel(self) -> str:
        return self.params_pubsub_channel()

    def global_params_pubsub_channel(self) -> str:
        return f"{self.prefix}:params:global"

    @classmethod
    def global_params_channel(
        cls,
        *,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        safe_namespace = validate_identifier_part(namespace, "namespace")
        safe_schema_version = validate_schema_version(schema_version, "schema_version")
        return f"{safe_namespace}:{safe_schema_version}:params:global"

    def inbound_stream(self, environment: str, topic: str) -> str:
        safe_environment = validate_identifier_part(environment, "environment")
        safe_topic = validate_identifier_part(topic, "topic")
        return f"{self.prefix}:in:stream:{safe_environment}:{self._strategy_id}:{safe_topic}"
