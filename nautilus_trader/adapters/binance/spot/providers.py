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

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceSymbolFilter
from nautilus_trader.adapters.binance.common.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.adapters.binance.spot.http.market import BinanceSpotMarketHttpAPI
from nautilus_trader.adapters.binance.spot.http.wallet import BinanceSpotWalletHttpAPI
from nautilus_trader.adapters.binance.spot.schemas.market import BinanceSpotSymbolInfo
from nautilus_trader.adapters.binance.spot.schemas.wallet import BinanceSpotTradeFee
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BinanceSpotInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading instruments from the Binance Spot/Margin exchange.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    clock : LiveClock
        The clock for the provider.
    account_type : BinanceAccountType, default SPOT
        The Binance account type for the provider.
    is_testnet : bool, default False
        If the provider is for the Spot testnet.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.SPOT,
        is_testnet: bool = False,
        config: InstrumentProviderConfig | None = None,
        venue: Venue = BINANCE_VENUE,
    ) -> None:
        super().__init__(config=config)

        self._clock = clock
        self._client = client
        self._account_type = account_type
        self._is_testnet = is_testnet
        self._venue = venue

        self._http_wallet = BinanceSpotWalletHttpAPI(
            self._client,
            clock=self._clock,
            account_type=account_type,
        )
        self._http_market = BinanceSpotMarketHttpAPI(self._client, account_type=account_type)

        self._log_warnings = config.log_warnings if config else True

        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        try:
            # Get current commission rates
            if not self._is_testnet:
                response = await self._http_wallet.query_spot_trade_fees()
                fees_dict: dict[str, BinanceSpotTradeFee] = {fee.symbol: fee for fee in response}
            else:
                self._log.warning(
                    "Currently not requesting actual trade fees for the SPOT testnet; "
                    "all instruments will have zero fees",
                )
                fees_dict = {}
        except BinanceClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to request the applicable account fee tier). {e.message}",
            )
            return

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_spot_exchange_info()
        for symbol_info in exchange_info.symbols:
            self._parse_instrument(
                symbol_info=symbol_info,
                fee=fees_dict.get(symbol_info.symbol),
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading.")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, self._venue, "instrument_id.venue", "BINANCE")

        try:
            # Get current commission rates
            if not self._is_testnet:
                response = await self._http_wallet.query_spot_trade_fees()
                fees_dict: dict[str, BinanceSpotTradeFee] = {fee.symbol: fee for fee in response}
            else:
                fees_dict = {}
                self._log.warning(
                    "Currently not requesting actual trade fees for the SPOT testnet; "
                    "all instruments will have zero fees.",
                )
        except BinanceClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to request the applicable account fee tier): {e.message}",
            )
            return

        # Extract all symbol strings
        symbols = [
            str(BinanceSymbol(instrument_id.symbol.value)) for instrument_id in instrument_ids
        ]
        # Get exchange info for all assets
        exchange_info = await self._http_market.query_spot_exchange_info(symbols=symbols)
        symbol_info_dict: dict[str, BinanceSpotSymbolInfo] = {
            info.symbol: info for info in exchange_info.symbols
        }

        for symbol in symbols:
            self._parse_instrument(
                symbol_info=symbol_info_dict[symbol],
                fee=fees_dict.get(symbol),
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self._venue, "instrument_id.venue", "BINANCE")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}")

        symbol = str(BinanceSymbol(instrument_id.symbol.value))

        try:
            # Get current commission rates
            if not self._is_testnet:
                response = await self._http_wallet.query_spot_trade_fees(symbol=symbol)
                fees_dict: dict[str, BinanceSpotTradeFee] = {fee.symbol: fee for fee in response}
            else:
                self._log.warning(
                    "Currently not requesting actual trade fees for the SPOT testnet; "
                    "all instruments will have zero fees",
                )
                fees_dict = {}
        except BinanceClientError as e:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                f"(this is needed to request the applicable account fee tier): {e}",
            )
            return

        # Get exchange info for asset
        exchange_info = await self._http_market.query_spot_exchange_info(symbol=symbol)
        symbol_info_dict: dict[str, BinanceSpotSymbolInfo] = {
            info.symbol: info for info in exchange_info.symbols
        }

        self._parse_instrument(
            symbol_info=symbol_info_dict[symbol],
            fee=fees_dict.get(symbol),
            ts_event=millis_to_nanos(exchange_info.serverTime),
        )

    def _parse_instrument(
        self,
        symbol_info: BinanceSpotSymbolInfo,
        fee: BinanceSpotTradeFee | None,
        ts_event: int,
    ) -> None:
        ts_init = self._clock.timestamp_ns()
        try:
            base_currency = symbol_info.parse_to_base_asset()
            quote_currency = symbol_info.parse_to_quote_asset()

            raw_symbol = Symbol(symbol_info.symbol)
            instrument_id = InstrumentId(symbol=raw_symbol, venue=self._venue)

            # Parse instrument filters
            filters: dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
                f.filterType: f for f in symbol_info.filters
            }
            price_filter = filters[BinanceSymbolFilterType.PRICE_FILTER]
            lot_size_filter = filters[BinanceSymbolFilterType.LOT_SIZE]

            min_notional_filter = filters.get(BinanceSymbolFilterType.MIN_NOTIONAL)
            notional_filter = filters.get(BinanceSymbolFilterType.NOTIONAL)

            tick_size = price_filter.tickSize
            step_size = lot_size_filter.stepSize
            PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
            PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")

            price_precision = abs(int(Decimal(tick_size).as_tuple().exponent))
            size_precision = abs(int(Decimal(step_size).as_tuple().exponent))
            price_increment = Price.from_str(tick_size)
            size_increment = Quantity.from_str(step_size)
            lot_size = Quantity.from_str(step_size)

            PyCondition.in_range(
                Decimal(lot_size_filter.maxQty),
                QUANTITY_MIN,
                QUANTITY_MAX,
                "maxQty",
            )
            PyCondition.in_range(
                Decimal(lot_size_filter.minQty),
                QUANTITY_MIN,
                QUANTITY_MAX,
                "minQty",
            )

            max_quantity = Quantity(float(lot_size_filter.maxQty), precision=size_precision)
            min_quantity = Quantity(float(lot_size_filter.minQty), precision=size_precision)

            max_notional = None
            min_notional = None
            if min_notional_filter:
                min_notional = Money(min_notional_filter.minNotional, currency=quote_currency)
            elif notional_filter:
                max_notional = Money(notional_filter.maxNotional, currency=quote_currency)
                min_notional = Money(notional_filter.minNotional, currency=quote_currency)

            max_price = Price(
                min(float(price_filter.maxPrice), 4294967296.0),
                precision=price_precision,
            )
            min_price = Price(max(float(price_filter.minPrice), 0.0), precision=price_precision)

            # Parse fees
            maker_fee: Decimal = Decimal(0)
            taker_fee: Decimal = Decimal(0)
            if fee:
                assert fee.symbol == symbol_info.symbol
                maker_fee = Decimal(fee.makerCommission)
                taker_fee = Decimal(fee.takerCommission)

            # Create instrument
            instrument = CurrencyPair(
                instrument_id=instrument_id,
                raw_symbol=raw_symbol,
                base_currency=base_currency,
                quote_currency=quote_currency,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                lot_size=lot_size,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=max_notional,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal(0),
                margin_maint=Decimal(0),
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=min(ts_event, ts_init),
                ts_init=ts_init,
                info=msgspec.structs.asdict(symbol_info),
            )
            self.add_currency(currency=instrument.base_currency)
            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)

            self._log.debug(f"Added instrument {instrument.id}.")
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {symbol_info.symbol}: {e}.")
