# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from datetime import datetime as dt
from decimal import Decimal
from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.constants import BINANCE_VENUE
from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinanceSymbolFilterType
from nautilus_trader.adapters.binance.common.schemas.market import BinanceSymbolFilter
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesContractStatus
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesContractType
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.http.wallet import BinanceFuturesWalletHttpAPI
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesSymbolInfo
from nautilus_trader.adapters.binance.futures.schemas.wallet import BinanceFuturesCommissionRate
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.error import BinanceClientError
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.crypto_future import CryptoFuture
from nautilus_trader.model.instruments.crypto_perpetual import CryptoPerpetual
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BinanceFuturesInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading instruments from the `Binance Futures` exchange.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    logger : Logger
        The logger for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.
    """

    def __init__(
        self,
        client: BinanceHttpClient,
        logger: Logger,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.FUTURES_USDT,
        config: Optional[InstrumentProviderConfig] = None,
    ):
        super().__init__(
            venue=BINANCE_VENUE,
            logger=logger,
            config=config,
        )

        self._client = client
        self._account_type = account_type
        self._clock = clock

        self._http_wallet = BinanceFuturesWalletHttpAPI(
            self._client,
            clock=self._clock,
            account_type=account_type,
        )
        self._http_market = BinanceFuturesMarketHttpAPI(self._client, account_type=account_type)

        self._log_warnings = config.log_warnings if config else True

        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: Optional[dict] = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_futures_exchange_info()

        self._log.warning(
            "Currently not requesting actual trade fees. All instruments will have zero fees.",
        )
        for symbol_info in exchange_info.symbols:
            fee: Optional[BinanceFuturesCommissionRate] = None
            # TODO(cs): This won't work for 174 instruments, we'll have to pre-request these
            #  in some other way.
            # if not self._client.base_url.__contains__("testnet.binancefuture.com"):
            #     try:
            #         # Get current commission rates for the symbol
            #         fee = await self._http_wallet.query_futures_commission_rate(symbol_info.symbol)
            #         print(fee)
            #     except BinanceClientError as e:
            #         self._log.error(
            #             "Cannot load instruments: API key authentication failed "
            #             f"(this is needed to fetch the applicable account fee tier). {e.message}",
            #         )
            #         return

            self._parse_instrument(
                symbol_info=symbol_info,
                fee=fee,
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: Optional[dict] = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading.")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading instruments {instrument_ids}{filters_str}.")

        # Extract all symbol strings
        symbols = [
            str(BinanceSymbol(instrument_id.symbol.value)) for instrument_id in instrument_ids
        ]

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_futures_exchange_info()
        symbol_info_dict: dict[str, BinanceFuturesSymbolInfo] = {
            info.symbol: info for info in exchange_info.symbols
        }

        self._log.warning(
            "Currently not requesting actual trade fees. All instruments will have zero fees.",
        )
        for symbol in symbols:
            fee: Optional[BinanceFuturesCommissionRate] = None
            # TODO(cs): This won't work for 174 instruments, we'll have to pre-request these
            #  in some other way.
            # if not self._client.base_url.__contains__("testnet.binancefuture.com"):
            #     try:
            #         # Get current commission rates for the symbol
            #         fee = await self._http_wallet.query_futures_commission_rate(symbol)
            #     except BinanceClientError as e:
            #         self._log.error(
            #             "Cannot load instruments: API key authentication failed "
            #             f"(this is needed to fetch the applicable account fee tier). {e.message}",
            #         )

            self._parse_instrument(
                symbol_info=symbol_info_dict[symbol],
                fee=fee,
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[dict] = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}.")

        symbol = str(BinanceSymbol(instrument_id.symbol.value))

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_futures_exchange_info()
        symbol_info_dict: dict[str, BinanceFuturesSymbolInfo] = {
            info.symbol: info for info in exchange_info.symbols
        }

        fee: Optional[BinanceFuturesCommissionRate] = None
        if not self._client.base_url.__contains__("testnet.binancefuture.com"):
            try:
                # Get current commission rates for the symbol
                fee = await self._http_wallet.query_futures_commission_rate(symbol)
            except BinanceClientError as e:
                self._log.error(
                    "Cannot load instruments: API key authentication failed "
                    f"(this is needed to fetch the applicable account fee tier). {e.message}",
                )

        self._parse_instrument(
            symbol_info=symbol_info_dict[symbol],
            ts_event=millis_to_nanos(exchange_info.serverTime),
            fee=fee,
        )

    def _parse_instrument(  # noqa (C901 too complex)
        self,
        symbol_info: BinanceFuturesSymbolInfo,
        ts_event: int,
        fee: Optional[BinanceFuturesCommissionRate] = None,
    ) -> None:
        contract_type_str = symbol_info.contractType

        if (
            contract_type_str == ""
            or symbol_info.status == BinanceFuturesContractStatus.PENDING_TRADING
        ):
            self._log.debug(f"Instrument not yet defined: {symbol_info.symbol}")
            return  # Not yet defined

        ts_init = self._clock.timestamp_ns()
        try:
            # Create quote and base assets
            base_currency = symbol_info.parse_to_base_currency()
            quote_currency = symbol_info.parse_to_quote_currency()

            binance_symbol = BinanceSymbol(symbol_info.symbol).parse_binance_to_internal(
                self._account_type,
            )
            native_symbol = Symbol(binance_symbol)
            instrument_id = InstrumentId(symbol=native_symbol, venue=BINANCE_VENUE)

            # Parse instrument filters
            filters: dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
                f.filterType: f for f in symbol_info.filters
            }
            price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
            lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
            min_notional_filter: BinanceSymbolFilter = filters.get(
                BinanceSymbolFilterType.MIN_NOTIONAL,
            )

            tick_size = price_filter.tickSize.rstrip("0")
            step_size = lot_size_filter.stepSize.rstrip("0")
            PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
            PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")

            price_precision = abs(int(Decimal(tick_size).as_tuple().exponent))
            size_precision = abs(int(Decimal(step_size).as_tuple().exponent))
            price_increment = Price.from_str(tick_size)
            size_increment = Quantity.from_str(step_size)
            max_quantity = Quantity(float(lot_size_filter.maxQty), precision=size_precision)
            min_quantity = Quantity(float(lot_size_filter.minQty), precision=size_precision)
            min_notional = None
            if filters.get(BinanceSymbolFilterType.MIN_NOTIONAL):
                min_notional = Money(min_notional_filter.minNotional, currency=quote_currency)
            max_price = Price(float(price_filter.maxPrice), precision=price_precision)
            min_price = Price(float(price_filter.minPrice), precision=price_precision)

            # Futures commissions
            maker_fee = Decimal(0)
            taker_fee = Decimal(0)
            if fee:
                assert fee.symbol == symbol_info.symbol
                maker_fee = Decimal(fee.makerCommissionRate)
                taker_fee = Decimal(fee.takerCommissionRate)

            if symbol_info.marginAsset == symbol_info.baseAsset:
                settlement_currency = base_currency
            elif symbol_info.marginAsset == symbol_info.quoteAsset:
                settlement_currency = quote_currency
            else:
                raise ValueError(f"Unrecognized margin asset {symbol_info.marginAsset}")

            contract_type = BinanceFuturesContractType(contract_type_str)
            if contract_type == BinanceFuturesContractType.PERPETUAL:
                instrument = CryptoPerpetual(
                    instrument_id=instrument_id,
                    native_symbol=native_symbol,
                    base_currency=base_currency,
                    quote_currency=quote_currency,
                    settlement_currency=settlement_currency,
                    is_inverse=False,  # No inverse instruments trade on Binance
                    price_precision=price_precision,
                    size_precision=size_precision,
                    price_increment=price_increment,
                    size_increment=size_increment,
                    max_quantity=max_quantity,
                    min_quantity=min_quantity,
                    max_notional=None,
                    min_notional=min_notional,
                    max_price=max_price,
                    min_price=min_price,
                    margin_init=Decimal(float(symbol_info.requiredMarginPercent) / 100),
                    margin_maint=Decimal(float(symbol_info.maintMarginPercent) / 100),
                    maker_fee=maker_fee,
                    taker_fee=taker_fee,
                    ts_event=ts_event,
                    ts_init=ts_init,
                    info=self._decoder.decode(self._encoder.encode(symbol_info)),
                )
                self.add_currency(currency=instrument.base_currency)
            elif contract_type in (
                BinanceFuturesContractType.CURRENT_MONTH,
                BinanceFuturesContractType.CURRENT_QUARTER,
                BinanceFuturesContractType.NEXT_MONTH,
                BinanceFuturesContractType.NEXT_QUARTER,
            ):
                instrument = CryptoFuture(
                    instrument_id=instrument_id,
                    native_symbol=native_symbol,
                    underlying=base_currency,
                    quote_currency=quote_currency,
                    settlement_currency=settlement_currency,
                    expiry_date=dt.strptime(symbol_info.symbol.partition("_")[2], "%y%m%d").date(),
                    price_precision=price_precision,
                    size_precision=size_precision,
                    price_increment=price_increment,
                    size_increment=size_increment,
                    max_quantity=max_quantity,
                    min_quantity=min_quantity,
                    max_notional=None,
                    min_notional=min_notional,
                    max_price=max_price,
                    min_price=min_price,
                    margin_init=Decimal(float(symbol_info.requiredMarginPercent) / 100),
                    margin_maint=Decimal(float(symbol_info.maintMarginPercent) / 100),
                    maker_fee=maker_fee,
                    taker_fee=taker_fee,
                    ts_event=ts_event,
                    ts_init=ts_init,
                    info=self._decoder.decode(self._encoder.encode(symbol_info)),
                )
                self.add_currency(currency=instrument.underlying)
            else:
                raise RuntimeError(  # pragma: no cover (design-time error)
                    f"invalid `BinanceFuturesContractType`, was {contract_type}",  # pragma: no cover
                )

            self.add_currency(currency=instrument.quote_currency)
            self.add(instrument=instrument)

            self._log.debug(f"Added instrument {instrument.id}.")
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {symbol_info.symbol}, {e}.")
