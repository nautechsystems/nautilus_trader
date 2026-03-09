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

from dataclasses import dataclass

import msgspec

from nautilus_trader.adapters.pancakeswap.constants import PANCAKESWAP_V2_BSC_VENUE
from nautilus_trader.adapters.pancakeswap.constants import get_pancakeswap_v2_defaults
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue


def _require_blockchain_bindings() -> object:
    blockchain_module = getattr(nautilus_pyo3, "blockchain", None)
    if blockchain_module is None:
        raise RuntimeError(
            "PancakeSwap execution bindings require `nautilus_pyo3` built with the `defi` feature",
        )
    return blockchain_module


@dataclass(frozen=True)
class PancakeSwapV2ResolvedAddresses:
    """Resolved address bundle used for execution configuration."""

    router_address: str
    factory_address: str
    wnative_address: str


class PancakeSwapV2ExecClientConfig(LiveExecClientConfig, frozen=True, kw_only=True):
    """
    User-facing PancakeSwap V2 execution config wrapper.

    This wrapper resolves BSC defaults from Rust exports, validates signer-execution
    requirements, then builds a PyO3 ``BlockchainExecutionClientConfig``.

    Parameters
    ----------
    trader_id : TraderId
        Trader identifier for the execution client.
    client_id : AccountId
        Account/client identifier for the execution client.
    wallet_address : str
        EVM wallet address tracked and used for execution ownership.
    http_rpc_url : str
        HTTP RPC URL for node connectivity.
    signer_endpoint : str
        Remote signer service base URL.
    chain_id : int, default 56
        EVM chain id used for defaults and chain resolution.
    venue : Venue, default Venue("Bsc:PancakeSwapV2")
        Routing venue for execution engine venue mapping.
    tokens : tuple[str, ...], optional
        Explicit execution token universe.
    wallet_extra_tokens : tuple[str, ...], optional
        Additional wallet snapshot tokens.
    wallet_allowance_spenders : tuple[str, ...], optional
        Additional spender addresses to track allowances for.
    router_address : str, optional
        Optional router override. Requires ``allow_unsafe_address_override=True`` when
        it differs from canonical chain defaults.
    factory_address : str, optional
        Optional factory override. Requires ``allow_unsafe_address_override=True`` when
        it differs from canonical chain defaults.
    wnative_address : str, optional
        Optional wrapped-native override. Requires ``allow_unsafe_address_override=True``
        when it differs from canonical chain defaults.
    allow_unsafe_address_override : bool, default False
        Allows explicit router/factory/WBNB overrides that diverge from Rust defaults.

    Notes
    -----
    This wrapper is signer-only and sets ``execution_require_preapproved_allowance`` to
    ``True`` by default. Approval automation can be enabled explicitly by setting it to
    ``False``.

    """

    trader_id: TraderId
    client_id: AccountId
    wallet_address: str
    http_rpc_url: str
    signer_endpoint: str

    chain_id: int = 56
    venue: Venue = PANCAKESWAP_V2_BSC_VENUE

    tokens: tuple[str, ...] = ()
    wallet_extra_tokens: tuple[str, ...] = ()
    wallet_allowance_spenders: tuple[str, ...] = ()

    wallet_snapshot_ttl_secs: int = 30
    wallet_max_tokens_per_refresh: int = 256
    wallet_refresh_on_connect: bool = True
    multicall_max_batch_size: int = 64
    multicall_min_batch_size: int = 4

    signer_route: str = "/sign/eth"
    signer_timeout_ms: int = 5_000
    signer_require_tls: bool = True
    signer_wallet_address: str | None = None

    router_address: str | None = None
    factory_address: str | None = None
    wnative_address: str | None = None
    allow_unsafe_address_override: bool = False

    execution_default_slippage_bps: int = 100
    execution_default_deadline_secs: int = 120
    execution_confirmations_required: int = 1
    execution_receipt_max_polls: int = 60
    execution_receipt_poll_interval_ms: int = 1_000
    execution_max_inflight_txs_per_wallet: int = 1
    execution_require_preapproved_allowance: bool = True
    execution_max_fee_per_gas: int = 1_000_000_000
    execution_max_priority_fee_per_gas: int = 1_000_000_000
    execution_journal_path: str | None = None
    execution_unsupported_token_addresses: tuple[str, ...] = ()

    rpc_requests_per_second: int | None = None

    def __post_init__(self) -> None:
        self._validate_required_fields()
        self._validate_numeric_fields()

        resolved = self.resolve_addresses()
        self._validate_venue()
        self._validate_override_policy(resolved)

    def _validate_required_fields(self) -> None:
        PyCondition.not_none(self.trader_id, "trader_id")
        PyCondition.not_none(self.client_id, "client_id")
        PyCondition.not_none(self.venue, "venue")

        PyCondition.valid_string(self.wallet_address, "wallet_address")
        PyCondition.valid_string(self.http_rpc_url, "http_rpc_url")
        PyCondition.valid_string(self.signer_endpoint, "signer_endpoint")
        PyCondition.valid_string(self.signer_route, "signer_route")

    def _validate_numeric_fields(self) -> None:
        self._validate_positive("chain_id", self.chain_id)
        self._validate_positive("signer_timeout_ms", self.signer_timeout_ms)
        self._validate_positive("wallet_snapshot_ttl_secs", self.wallet_snapshot_ttl_secs)
        self._validate_positive(
            "wallet_max_tokens_per_refresh",
            self.wallet_max_tokens_per_refresh,
        )
        self._validate_positive("multicall_max_batch_size", self.multicall_max_batch_size)
        self._validate_positive("multicall_min_batch_size", self.multicall_min_batch_size)

        if self.multicall_min_batch_size > self.multicall_max_batch_size:
            raise ValueError("multicall_min_batch_size cannot exceed multicall_max_batch_size")

        self._validate_positive(
            "execution_default_slippage_bps",
            self.execution_default_slippage_bps,
        )
        self._validate_positive(
            "execution_default_deadline_secs",
            self.execution_default_deadline_secs,
        )
        self._validate_positive(
            "execution_confirmations_required",
            self.execution_confirmations_required,
        )
        self._validate_positive("execution_receipt_max_polls", self.execution_receipt_max_polls)
        self._validate_positive(
            "execution_receipt_poll_interval_ms",
            self.execution_receipt_poll_interval_ms,
        )
        self._validate_positive(
            "execution_max_inflight_txs_per_wallet",
            self.execution_max_inflight_txs_per_wallet,
        )
        self._validate_positive("execution_max_fee_per_gas", self.execution_max_fee_per_gas)
        self._validate_positive(
            "execution_max_priority_fee_per_gas",
            self.execution_max_priority_fee_per_gas,
        )

        if self.rpc_requests_per_second is not None and self.rpc_requests_per_second <= 0:
            raise ValueError("rpc_requests_per_second must be greater than zero when provided")

    def _validate_venue(self) -> None:
        venue_value = self.venue.value
        if ":" not in venue_value:
            raise ValueError(f"venue must be DEX format '<Chain>:<DexType>', was '{venue_value}'")
        if venue_value.partition(":")[2] != "PancakeSwapV2":
            raise ValueError(
                f"venue dex type must be 'PancakeSwapV2', was '{venue_value.partition(':')[2]}'",
            )

    def _validate_override_policy(self, resolved: PancakeSwapV2ResolvedAddresses) -> None:
        if not self.allow_unsafe_address_override:
            defaults = get_pancakeswap_v2_defaults(self.chain_id)
            if resolved.router_address.lower() != defaults.router_address.lower():
                raise ValueError(
                    "router_address differs from canonical Rust defaults; set "
                    "allow_unsafe_address_override=True to proceed",
                )
            if resolved.factory_address.lower() != defaults.factory_address.lower():
                raise ValueError(
                    "factory_address differs from canonical Rust defaults; set "
                    "allow_unsafe_address_override=True to proceed",
                )
            if resolved.wnative_address.lower() != defaults.wnative_address.lower():
                raise ValueError(
                    "wnative_address differs from canonical Rust defaults; set "
                    "allow_unsafe_address_override=True to proceed",
                )

    @staticmethod
    def _validate_positive(field: str, value: int) -> None:
        if value <= 0:
            raise ValueError(f"{field} must be greater than zero")

    def resolve_addresses(self) -> PancakeSwapV2ResolvedAddresses:
        """Resolve router/factory/WBNB with explicit config precedence over Rust defaults."""
        defaults = get_pancakeswap_v2_defaults(self.chain_id)
        return PancakeSwapV2ResolvedAddresses(
            router_address=self.router_address or defaults.router_address,
            factory_address=self.factory_address or defaults.factory_address,
            wnative_address=self.wnative_address or defaults.wnative_address,
        )

    def to_pyo3(self) -> nautilus_pyo3.blockchain.BlockchainExecutionClientConfig:
        """Build the canonical PyO3 blockchain execution config."""
        blockchain_module = _require_blockchain_bindings()
        chain = nautilus_pyo3.Chain.from_chain_id(self.chain_id)
        if chain is None:
            raise ValueError(f"Unsupported chain_id {self.chain_id}")

        resolved = self.resolve_addresses()

        spender_addresses = list(
            dict.fromkeys((*self.wallet_allowance_spenders, resolved.router_address))
        )
        wallet_extra_tokens = tuple(
            dict.fromkeys((*self.wallet_extra_tokens, resolved.wnative_address))
        )
        merged_tokens = tuple(dict.fromkeys((*self.tokens, *wallet_extra_tokens)))

        return blockchain_module.BlockchainExecutionClientConfig(
            trader_id=self.trader_id,
            client_id=self.client_id,
            venue=self.venue,
            chain=chain,
            wallet_address=self.wallet_address,
            http_rpc_url=self.http_rpc_url,
            tokens=list(merged_tokens) if merged_tokens else None,
            rpc_requests_per_second=self.rpc_requests_per_second,
            wallet_extra_tokens=list(wallet_extra_tokens),
            wallet_wnative_address=resolved.wnative_address,
            wallet_allowance_spenders=spender_addresses,
            wallet_snapshot_ttl_secs=self.wallet_snapshot_ttl_secs,
            wallet_max_tokens_per_refresh=self.wallet_max_tokens_per_refresh,
            wallet_refresh_on_connect=self.wallet_refresh_on_connect,
            multicall_max_batch_size=self.multicall_max_batch_size,
            multicall_min_batch_size=self.multicall_min_batch_size,
            signer_endpoint=self.signer_endpoint,
            signer_route=self.signer_route,
            signer_timeout_ms=self.signer_timeout_ms,
            signer_require_tls=self.signer_require_tls,
            signer_wallet_address=self.signer_wallet_address,
            execution_router_address=resolved.router_address,
            execution_default_slippage_bps=self.execution_default_slippage_bps,
            execution_default_deadline_secs=self.execution_default_deadline_secs,
            execution_confirmations_required=self.execution_confirmations_required,
            execution_receipt_max_polls=self.execution_receipt_max_polls,
            execution_receipt_poll_interval_ms=self.execution_receipt_poll_interval_ms,
            execution_max_inflight_txs_per_wallet=self.execution_max_inflight_txs_per_wallet,
            execution_require_preapproved_allowance=self.execution_require_preapproved_allowance,
            execution_max_fee_per_gas=self.execution_max_fee_per_gas,
            execution_max_priority_fee_per_gas=self.execution_max_priority_fee_per_gas,
            execution_journal_path=self.execution_journal_path,
            execution_unsupported_token_addresses=list(self.execution_unsupported_token_addresses),
        )

    @classmethod
    def from_dict(cls, values: dict[str, object]) -> PancakeSwapV2ExecClientConfig:
        """Decode from a plain dictionary (for config file driven workflows)."""
        return msgspec.convert(values, type=cls)
