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

from decimal import Decimal

import msgspec

from nautilus_trader.adapters.okx.common.constants import OKX_VENUE
from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.symbol import OKXSymbol
from nautilus_trader.adapters.okx.http.account import OKXAccountHttpAPI
from nautilus_trader.adapters.okx.http.client import OKXHttpClient
from nautilus_trader.adapters.okx.http.public import OKXPublicHttpAPI
from nautilus_trader.adapters.okx.schemas.account.trade_fee import OKXTradeFee
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrument
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentList
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentSpot
from nautilus_trader.adapters.okx.schemas.public.instrument import OKXInstrumentSwap
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument


class OKXInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from OKX.

    Parameters
    ----------
    client : OKXHttpClient
        The OKX HTTP client.
    clock : LiveClock
        The clock instance.
    instrument_types : tuple[OKXInstrumentType]
        The instrument types to load. Must be compatible with `contract_types`.
    contract_types : tuple[OKXContractType], optional
        The contract types of instruments to load. The default is all contract types, i.e.,
        `OKXInstrumentType.LINEAR`, `OKXInstrumentType.INVERSE`, and `OKXInstrumentType.NONE`. Must
        be compatible with `instrument_types`.
    config : InstrumentProviderConfig, optional
        The instrument provider configuration, by default None.

    """

    def __init__(
        self,
        client: OKXHttpClient,
        clock: LiveClock,
        instrument_types: tuple[OKXInstrumentType],
        contract_types: tuple[OKXContractType] = tuple(OKXContractType),  # type: ignore
        config: InstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._clock = clock
        self._client = client
        self._instrument_types = instrument_types
        self._contract_types = contract_types

        # Validate instrument and contract types and their compatibility
        self._validate_instrument_and_contract_types()

        self._http_public = OKXPublicHttpAPI(
            client=client,
            clock=clock,
        )

        self._http_account = OKXAccountHttpAPI(
            client=client,
            clock=clock,
        )

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

        # Hot cache instrument type fee rates (making InstrumentProvider also the fee rate provider)
        self._fee_rates: dict[OKXInstrumentType, OKXTradeFee] = {}

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        instrument_infos: dict[tuple[OKXInstrumentType, OKXContractType], OKXInstrumentList] = {}

        for instrument_type in self._instrument_types:
            self._fee_rates[instrument_type] = await self._http_account.fetch_trade_fee(
                instrument_type,
            )
            for contract_type in self._contract_types:
                instrument_infos[(instrument_type, contract_type)] = (
                    await self._http_public.fetch_instruments(instrument_type, contract_type)
                )

        for instrument_type, contract_type in instrument_infos:
            for instrument in instrument_infos[(instrument_type, contract_type)]:
                trade_fee = self._fee_rates.get(instrument_type, None)
                if trade_fee:
                    self._parse_instrument(instrument, trade_fee)
                else:
                    self._log.warning(
                        f"Unable to find trade fee for instrument {instrument}",
                    )
        self._log.info(f"Loaded {len(self._instruments)} instruments")

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, OKX_VENUE, "instrument_id.venue", "OKX")

        # Reupdate all fee rates
        for instrument_type in self._instrument_types:
            self._fee_rates[instrument_type] = await self._http_account.fetch_trade_fee(
                instrument_type,
            )

            # Extract symbol strings and product types
            for instrument_id in instrument_ids:
                okx_symbol = OKXSymbol(instrument_id.symbol.value)
                instrument = await self._http_public.fetch_instrument(
                    instType=okx_symbol.instrument_type,
                    instId=okx_symbol.raw_symbol,
                )
                trade_fee = self._fee_rates.get(instrument_type, None)
                if trade_fee:
                    self._parse_instrument(instrument, trade_fee)
                else:
                    self._log.warning(
                        f"Unable to find trade fee for instrument {instrument}",
                    )

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)

    def find_conditional(
        self,
        raw_symbol: str | None,
        instrument_type: OKXInstrumentType | None = None,
        contract_type: OKXContractType | None = None,
    ) -> Instrument | None:
        instruments = self.list_all()
        if not raw_symbol and not instrument_type and not contract_type:
            return instruments

        filter_strs = []
        if raw_symbol:
            filter_strs.append(f"`raw_symbol` {raw_symbol}")
            instruments = [i for i in instruments if i.info["instId"] == raw_symbol]

        if instrument_type:
            filter_strs.append(f"`instrument_type` {instrument_type}")
            instruments = [
                i
                for i in instruments
                if OKXInstrumentType[i.info["instType"]].value == instrument_type.value
            ]

        if contract_type:
            filter_strs.append(f"`contract_type` {contract_type}")
            instruments = [
                i
                for i in instruments
                if OKXContractType.find(i.info["ctType"]).value == contract_type.value
            ]

        if len(instruments) > 1:
            raise RuntimeError(
                "Found more than one instrument for filters: " + ", ".join(filter_strs),
            )
        return next(iter(instruments), None)  # allow none to be found

    def get_cached_fee_rate(self, instrument_type: OKXInstrumentType) -> OKXTradeFee | None:
        return self._fee_rates.get(instrument_type, None)

    def _parse_instrument(
        self,
        instrument: OKXInstrument,
        trade_fee: OKXTradeFee,
        margin_init: str | Decimal = Decimal("0.1"),
        margin_maint: str | Decimal = Decimal("0.1"),
    ) -> None:
        if isinstance(instrument, OKXInstrumentSwap):
            self._parse_swap_instrument(instrument, trade_fee)
        elif isinstance(instrument, OKXInstrumentSpot):
            self._parse_spot_instrument(instrument, trade_fee)
        else:
            raise TypeError(f"Unsupported (or Not Implemented) OKX instrument, was {instrument}")

    def _parse_swap_instrument(
        self,
        data: OKXInstrumentSwap,
        trade_fee: OKXTradeFee,
        margin_init: str | Decimal = Decimal("0.1"),
        margin_maint: str | Decimal = Decimal("0.1"),
    ) -> None:
        assert data.ctType in [OKXContractType.LINEAR, OKXContractType.INVERSE]
        if data.ctType == OKXContractType.LINEAR:
            assert data.settleCcy in [
                "USDT",
                "USDC",
            ], (
                "OKX linear swap instruments are expected to have settlement currencies of either "
                f"USDT or USDC - got {data.settleCcy}"
            )

        try:
            base_currency = self.currency(data.ctValCcy)
            quote_currency = self.currency(data.settleCcy)

            self.add_currency(base_currency)
            self.add_currency(quote_currency)

            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()

            if data.ctType == OKXContractType.LINEAR:
                maker_fee = trade_fee.makerU if data.settleCcy == "USDT" else trade_fee.makerUSDC
                taker_fee = trade_fee.takerU if data.settleCcy == "USDT" else trade_fee.takerUSDC
            else:
                maker_fee = trade_fee.maker
                taker_fee = trade_fee.taker

            instrument = data.parse_to_instrument(
                base_currency=base_currency,
                quote_currency=quote_currency,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                margin_init=margin_init,
                margin_maint=margin_maint,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            # self._log.info(f"Adding {instrument}")
            self.add(instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse swap instrument {data.instId}: {e}")

    def _parse_spot_instrument(
        self,
        data: OKXInstrumentSpot,
        trade_fee: OKXTradeFee,
        margin_init: str | Decimal = Decimal("0.1"),
        margin_maint: str | Decimal = Decimal("0.1"),
    ) -> None:
        try:
            base_currency = self.currency(data.baseCcy)
            quote_currency = self.currency(data.quoteCcy)

            self.add_currency(base_currency)
            self.add_currency(quote_currency)

            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()

            instrument = data.parse_to_instrument(
                base_currency=base_currency,
                quote_currency=quote_currency,
                maker_fee=trade_fee.maker,
                taker_fee=trade_fee.taker,
                margin_init=margin_init,
                margin_maint=margin_maint,
                ts_event=ts_event,
                ts_init=ts_init,
            )
            # self._log.info(f"Adding {instrument}")
            self.add(instrument)
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse spot instrument {data.instId}: {e}")

    def _validate_instrument_and_contract_types(self) -> None:
        assert (
            self._instrument_types and self._contract_types
        ), "`instrument_types` and `contract_types` cannot be empty"

        for instrument_type in self._instrument_types:
            allowed_contract_types = ALLOWED_INSTRUMENT_TYPE_CONTRACT_TYPE_COMBOS[instrument_type]
            if not set(self._contract_types).intersection(allowed_contract_types):
                raise RuntimeError(
                    f"Specified instrument type {instrument_type} is incompatible with specified "
                    f"contract types {self._contract_types}, allowed contract types for this "
                    f"instrument type are {allowed_contract_types}",
                )

        for contract_type in self._contract_types:
            allowed_inst_types = ALLOWED_CONTRACT_TYPE_INSTRUMENT_TYPE_COMBOS[contract_type]
            if not set(self._instrument_types).intersection(allowed_inst_types):
                raise RuntimeError(
                    f"Specified contract type {contract_type} is incompatible with specified "
                    f"instrument types {self._instrument_types}, allowed instrument types for this "
                    f"contract type are {allowed_inst_types}",
                )


ALLOWED_INSTRUMENT_TYPE_CONTRACT_TYPE_COMBOS = {
    OKXInstrumentType.ANY: list(OKXContractType),
    OKXInstrumentType.SPOT: [OKXContractType.NONE],
    OKXInstrumentType.MARGIN: [OKXContractType.NONE],
    OKXInstrumentType.OPTION: [OKXContractType.NONE],
    OKXInstrumentType.SWAP: [OKXContractType.LINEAR, OKXContractType.INVERSE],
    OKXInstrumentType.FUTURES: [OKXContractType.LINEAR, OKXContractType.INVERSE],
}
ALLOWED_CONTRACT_TYPE_INSTRUMENT_TYPE_COMBOS = {
    OKXContractType.NONE: [
        OKXInstrumentType.SPOT,
        OKXInstrumentType.MARGIN,
        OKXInstrumentType.OPTION,
    ],
    OKXContractType.LINEAR: [OKXInstrumentType.SWAP, OKXInstrumentType.FUTURES],
    OKXContractType.INVERSE: [OKXInstrumentType.SWAP, OKXInstrumentType.FUTURES],
}


def get_instrument_type_contract_type_combos(
    instrument_types: tuple[OKXInstrumentType],
    contract_types: tuple[OKXContractType] | None = None,
) -> list[tuple[OKXInstrumentType, OKXContractType]]:
    combos = []
    for i in instrument_types:
        allowed_contract_types = ALLOWED_INSTRUMENT_TYPE_CONTRACT_TYPE_COMBOS[i]
        if contract_types:
            for c in contract_types:
                if c in allowed_contract_types:
                    combos.append((i, c))
        else:
            for c in allowed_contract_types:
                combos.append((i, c))
    return combos
