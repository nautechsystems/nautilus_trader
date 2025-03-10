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
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesContractStatus
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesContractType
from nautilus_trader.adapters.binance.futures.http.account import BinanceFuturesAccountHttpAPI
from nautilus_trader.adapters.binance.futures.http.market import BinanceFuturesMarketHttpAPI
from nautilus_trader.adapters.binance.futures.http.wallet import BinanceFuturesWalletHttpAPI
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesFeeRates
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.adapters.binance.futures.schemas.market import BinanceFuturesSymbolInfo
from nautilus_trader.adapters.binance.futures.schemas.wallet import BinanceFuturesCommissionRate
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
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
    Provides a means of loading instruments from the Binance Futures exchange.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    config : InstrumentProviderConfig, optional
        The configuration for the provider.

    """

    def __init__(
        self,
        client: BinanceHttpClient,
        clock: LiveClock,
        account_type: BinanceAccountType = BinanceAccountType.USDT_FUTURE,
        config: InstrumentProviderConfig | None = None,
        venue: Venue = BINANCE_VENUE,
    ) -> None:
        super().__init__(config=config)

        self._clock = clock
        self._client = client
        self._account_type = account_type
        self._venue = venue

        self._http_account = BinanceFuturesAccountHttpAPI(
            self._client,
            clock=self._clock,
            account_type=account_type,
        )
        self._http_wallet = BinanceFuturesWalletHttpAPI(
            self._client,
            clock=self._clock,
            account_type=account_type,
        )
        self._http_market = BinanceFuturesMarketHttpAPI(self._client, account_type=account_type)

        self._log_warnings = config.log_warnings if config else True

        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

        # This fee rates map is only applicable for backtesting, as live trading will utilise
        # real-time account update messages provided by Binance.
        # These fee rates assume USD-M Futures Trading without the 10% off for using BNB or BUSD.
        # The next step is to enable users to pass their own fee rates map via the config.
        # In the future, we aim to represent this fee model with greater accuracy for backtesting.
        # https://www.binance.com/en/fee/futureFee
        self._fee_rates = {
            0: BinanceFuturesFeeRates(feeTier=0, maker="0.000200", taker="0.000500"),
            1: BinanceFuturesFeeRates(feeTier=1, maker="0.000160", taker="0.000400"),
            2: BinanceFuturesFeeRates(feeTier=2, maker="0.000140", taker="0.000350"),
            3: BinanceFuturesFeeRates(feeTier=3, maker="0.000120", taker="0.000320"),
            4: BinanceFuturesFeeRates(feeTier=4, maker="0.000100", taker="0.000300"),
            5: BinanceFuturesFeeRates(feeTier=5, maker="0.000080", taker="0.000270"),
            6: BinanceFuturesFeeRates(feeTier=6, maker="0.000060", taker="0.000250"),
            7: BinanceFuturesFeeRates(feeTier=7, maker="0.000040", taker="0.000220"),
            8: BinanceFuturesFeeRates(feeTier=8, maker="0.000020", taker="0.000200"),
            9: BinanceFuturesFeeRates(feeTier=9, maker="0.000000", taker="0.000170"),
        }

    async def load_all_async(self, filters: dict | None = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_futures_exchange_info()
        account_info = await self._http_account.query_futures_account_info(recv_window=str(5000))
        fee_rates = self._fee_rates[account_info.feeTier]

        for symbol_info in exchange_info.symbols:
            fee = BinanceFuturesCommissionRate(
                symbol=symbol_info.symbol,
                makerCommissionRate=fee_rates.maker,
                takerCommissionRate=fee_rates.taker,
            )

            self._parse_instrument(
                symbol_info=symbol_info,
                fee=fee,
                ts_event=millis_to_nanos(exchange_info.serverTime),
            )

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading.")
            return

        # Check all instrument IDs
        for instrument_id in instrument_ids:
            PyCondition.equal(instrument_id.venue, self._venue, "instrument_id.venue", "BINANCE")

        # Extract all symbol strings
        symbols = [
            str(BinanceSymbol(instrument_id.symbol.value)) for instrument_id in instrument_ids
        ]

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_futures_exchange_info()
        symbol_info_dict: dict[str, BinanceFuturesSymbolInfo] = {
            info.symbol: info for info in exchange_info.symbols
        }
        account_info = await self._http_account.query_futures_account_info(recv_window=str(5000))
        fee_rates = self._fee_rates[account_info.feeTier]

        position_risk_resp = await self._http_account.query_futures_position_risk()
        position_risk = {risk.symbol: risk for risk in position_risk_resp}
        for symbol in symbols:
            fee = BinanceFuturesCommissionRate(
                symbol=symbol,
                makerCommissionRate=fee_rates.maker,
                takerCommissionRate=fee_rates.taker,
            )
            # Fetch position risk
            if symbol not in position_risk:
                self._log.error(f"Position risk not found for {symbol}.")
                continue
            self._parse_instrument(
                symbol_info=symbol_info_dict[symbol],
                fee=fee,
                ts_event=millis_to_nanos(exchange_info.serverTime),
                position_risk=position_risk[symbol],
            )

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self._venue, "instrument_id.venue", "BINANCE")

        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.debug(f"Loading instrument {instrument_id}{filters_str}.")

        symbol = str(BinanceSymbol(instrument_id.symbol.value))

        # Get exchange info for all assets
        exchange_info = await self._http_market.query_futures_exchange_info()
        symbol_info_dict: dict[str, BinanceFuturesSymbolInfo] = {
            info.symbol: info for info in exchange_info.symbols
        }

        account_info = await self._http_account.query_futures_account_info(recv_window=str(5000))
        fee_rates = self._fee_rates[account_info.feeTier]
        fee = BinanceFuturesCommissionRate(
            symbol=symbol,
            makerCommissionRate=fee_rates.maker,
            takerCommissionRate=fee_rates.taker,
        )

        self._parse_instrument(
            symbol_info=symbol_info_dict[symbol],
            ts_event=millis_to_nanos(exchange_info.serverTime),
            fee=fee,
        )

    def _parse_instrument(
        self,
        symbol_info: BinanceFuturesSymbolInfo,
        ts_event: int,
        position_risk: BinanceFuturesPositionRisk | None = None,
        fee: BinanceFuturesCommissionRate | None = None,
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

            raw_symbol = Symbol(symbol_info.symbol)
            parsed_symbol = BinanceSymbol(raw_symbol.value).parse_as_nautilus(
                self._account_type,
            )
            nautilus_symbol = Symbol(parsed_symbol)
            instrument_id = InstrumentId(symbol=nautilus_symbol, venue=self._venue)

            # Parse instrument filters
            filters: dict[BinanceSymbolFilterType, BinanceSymbolFilter] = {
                f.filterType: f for f in symbol_info.filters
            }
            price_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.PRICE_FILTER)
            lot_size_filter: BinanceSymbolFilter = filters.get(BinanceSymbolFilterType.LOT_SIZE)
            min_notional_filter: BinanceSymbolFilter = filters.get(
                BinanceSymbolFilterType.MIN_NOTIONAL,
            )

            tick_size = price_filter.tickSize
            step_size = lot_size_filter.stepSize
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
                min_notional = Money(min_notional_filter.notional, currency=quote_currency)
            max_notional = (
                Money(position_risk.maxNotionalValue, currency=quote_currency)
                if position_risk
                else None
            )
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
                    raw_symbol=raw_symbol,
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
                    max_notional=max_notional,
                    min_notional=min_notional,
                    max_price=max_price,
                    min_price=min_price,
                    margin_init=Decimal(symbol_info.requiredMarginPercent) / 100,
                    margin_maint=Decimal(symbol_info.maintMarginPercent) / 100,
                    maker_fee=maker_fee,
                    taker_fee=taker_fee,
                    ts_event=ts_event,
                    ts_init=ts_init,
                    info=msgspec.structs.asdict(symbol_info),
                )
                self.add_currency(currency=instrument.base_currency)
            elif contract_type in (
                BinanceFuturesContractType.CURRENT_MONTH,
                BinanceFuturesContractType.CURRENT_QUARTER,
                BinanceFuturesContractType.CURRENT_QUARTER_DELIVERING,
                BinanceFuturesContractType.NEXT_MONTH,
                BinanceFuturesContractType.NEXT_QUARTER,
            ):
                instrument = CryptoFuture(
                    instrument_id=instrument_id,
                    raw_symbol=raw_symbol,
                    underlying=base_currency,
                    quote_currency=quote_currency,
                    settlement_currency=settlement_currency,
                    is_inverse=False,  # No inverse instruments trade on Binance
                    activation_ns=millis_to_nanos(symbol_info.onboardDate),
                    expiration_ns=millis_to_nanos(symbol_info.deliveryDate),
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
                    margin_init=Decimal(symbol_info.requiredMarginPercent) / 100,
                    margin_maint=Decimal(symbol_info.maintMarginPercent) / 100,
                    maker_fee=maker_fee,
                    taker_fee=taker_fee,
                    ts_event=ts_event,
                    ts_init=ts_init,
                    info=msgspec.structs.asdict(symbol_info),
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
                self._log.warning(f"Unable to parse instrument {symbol_info.symbol}: {e}.")
