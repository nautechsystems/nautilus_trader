from __future__ import annotations

from flux.common.config import FLUX_DEFAULT_NAMESPACE
from flux.common.config import FLUX_SCHEMA_VERSION
from flux.common.config import FluxIdentityConfig
from flux.common.config import validate_identifier_part
from flux.common.config import validate_schema_version
from flux.common.config import validate_symbol_part


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

    @classmethod
    def portfolio_inventory_component(
        cls,
        *,
        strategy_id: str,
        portfolio_id: str,
        base_currency: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        safe_namespace = validate_identifier_part(namespace, "namespace")
        safe_schema_version = validate_schema_version(schema_version, "schema_version")
        safe_strategy_id = validate_identifier_part(strategy_id, "strategy_id")
        safe_portfolio_id = validate_identifier_part(portfolio_id, "portfolio_id")
        safe_base_currency = validate_symbol_part(
            base_currency,
            "base_currency",
            allow_colon=True,
        ).upper()
        return (
            f"{safe_namespace}:{safe_schema_version}:portfolio:inventory:component:"
            f"{safe_portfolio_id}:{safe_base_currency}:{safe_strategy_id}"
        )

    @classmethod
    def portfolio_inventory(
        cls,
        *,
        portfolio_id: str,
        base_currency: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        safe_namespace = validate_identifier_part(namespace, "namespace")
        safe_schema_version = validate_schema_version(schema_version, "schema_version")
        safe_portfolio_id = validate_identifier_part(portfolio_id, "portfolio_id")
        safe_base_currency = validate_symbol_part(
            base_currency,
            "base_currency",
            allow_colon=True,
        ).upper()
        return (
            f"{safe_namespace}:{safe_schema_version}:portfolio:inventory:"
            f"{safe_portfolio_id}:{safe_base_currency}"
        )

    @classmethod
    def portfolio_inventory_channel(
        cls,
        *,
        portfolio_id: str,
        base_currency: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        return (
            cls.portfolio_inventory(
                portfolio_id=portfolio_id,
                base_currency=base_currency,
                namespace=namespace,
                schema_version=schema_version,
            )
            + ":changed"
        )

    @classmethod
    def portfolio_snapshot(
        cls,
        *,
        portfolio_id: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        safe_namespace = validate_identifier_part(namespace, "namespace")
        safe_schema_version = validate_schema_version(schema_version, "schema_version")
        safe_portfolio_id = validate_identifier_part(portfolio_id, "portfolio_id")
        return f"{safe_namespace}:{safe_schema_version}:portfolio:snapshot:{safe_portfolio_id}"

    @classmethod
    def portfolio_snapshot_channel(
        cls,
        *,
        portfolio_id: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        return (
            cls.portfolio_snapshot(
                portfolio_id=portfolio_id,
                namespace=namespace,
                schema_version=schema_version,
            )
            + ":changed"
        )

    @classmethod
    def profile_account_projection(
        cls,
        *,
        profile_id: str,
        account_scope_id: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        safe_namespace = validate_identifier_part(namespace, "namespace")
        safe_schema_version = validate_schema_version(schema_version, "schema_version")
        safe_profile_id = validate_identifier_part(profile_id, "profile_id")
        safe_account_scope_id = validate_identifier_part(account_scope_id, "account_scope_id")
        return (
            f"{safe_namespace}:{safe_schema_version}:profile:account_projection:"
            f"{safe_profile_id}:{safe_account_scope_id}"
        )

    @classmethod
    def profile_account_projection_channel(
        cls,
        *,
        profile_id: str,
        account_scope_id: str,
        namespace: str = FLUX_DEFAULT_NAMESPACE,
        schema_version: str = FLUX_SCHEMA_VERSION,
    ) -> str:
        return (
            cls.profile_account_projection(
                profile_id=profile_id,
                account_scope_id=account_scope_id,
                namespace=namespace,
                schema_version=schema_version,
            )
            + ":changed"
        )

    def market_last(
        self,
        exchange: str,
        base: str,
        quote: str,
        instrument_id: str | None = None,
    ) -> str:
        safe_exchange = validate_identifier_part(exchange, "exchange").lower()
        safe_instrument_id = (
            validate_symbol_part(instrument_id, "instrument_id", allow_colon=True).upper()
            if instrument_id
            else ""
        )
        if safe_instrument_id:
            return f"{self.prefix}:market:last:{self._strategy_id}:{safe_exchange}:{safe_instrument_id}"
        safe_base = validate_symbol_part(base, "base", allow_colon=True).upper()
        safe_quote = validate_symbol_part(quote, "quote", allow_colon=True).upper()
        return f"{self.prefix}:market:last:{self._strategy_id}:{safe_exchange}:{safe_base}_{safe_quote}"

    def params_hash_key(self) -> str:
        return f"{self.prefix}:params:{self._strategy_id}"

    def params_metadata_key(self) -> str:
        return f"{self.prefix}:params-meta:{self._strategy_id}"

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
