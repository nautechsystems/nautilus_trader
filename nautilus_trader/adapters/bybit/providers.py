from decimal import Decimal
from typing import Optional

import msgspec

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VENUE
from nautilus_trader.adapters.bybit.common.enums import BybitAccountType
from nautilus_trader.adapters.bybit.schemas.symbol import BybitSymbol
from nautilus_trader.adapters.bybit.http.client import BybitHttpClient
from nautilus_trader.adapters.bybit.http.market import BybitMarketHttpAPI
from nautilus_trader.adapters.bybit.schemas.market.instrument import BybitInstrument
from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import PRICE_MAX
from nautilus_trader.model.objects import PRICE_MIN
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitInstrumentProvider(InstrumentProvider):
    def __init__(
        self,
        client: BybitHttpClient,
        logger: Logger,
        clock: LiveClock,
        account_type: BybitAccountType,
        is_testnet: bool = False,
        config: Optional[InstrumentProviderConfig] = None,
    ):
        super().__init__(
            venue=BYBIT_VENUE,
            logger=logger,
            config=config,
        )
        self._clock = clock
        self._client = client
        self._account_type = account_type

        self._http_market = BybitMarketHttpAPI(
            client=client,
            clock=clock,
            account_type=account_type
        )

        self._log_warnings = config.log_warnings if config else True
        self._decoder = msgspec.json.Decoder()
        self._encoder = msgspec.json.Encoder()

    async def load_all_async(self, filters: Optional[dict] = None) -> None:
        filters_str = "..." if not filters else f" with filters {filters}..."
        self._log.info(f"Loading all instruments{filters_str}")

        instruments_info = await self._http_market.get_instruments_info()
        # risk_limits = await self._http_market.get_risk_limits()
        for instrument in instruments_info:
            self._parse_instrument(instrument)

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: Optional[dict] = None,
    ) -> None:
        if not instrument_ids:
            self._log.info("No instrument IDs given for loading.")
            return

    async def load_async(self, instrument_id: InstrumentId, filters: Optional[dict] = None) -> None:
        PyCondition.not_none(instrument_id, "instrument_id")
        PyCondition.equal(instrument_id.venue, self.venue, "instrument_id.venue", "self.venue")

    def _parse_instrument(
        self,
        instrument: BybitInstrument
    ):
        try:
            base_currency = instrument.parse_to_base_currency()
            quote_currency = instrument.parse_to_quote_currency()
            raw_symbol = Symbol(instrument.symbol)

            parsed_symbol = BybitSymbol(raw_symbol.value).parse_as_nautilus(self._account_type)
            nautilus_symbol = Symbol(parsed_symbol)
            instrument_id = InstrumentId(symbol=nautilus_symbol, venue=BYBIT_VENUE)
            if instrument.settleCoin == instrument.baseCoin:
                settlement_currency = base_currency
            elif instrument.settleCoin == instrument.quoteCoin:
                settlement_currency = quote_currency
            else:
                raise ValueError(f"Unrecognized margin asset {instrument.settleCoin}")

            tick_size = instrument.priceFilter.tickSize.rstrip("0")
            step_size = instrument.lotSizeFilter.qtyStep.rstrip("0")
            price_precision = abs(int(Decimal(instrument.priceFilter.tickSize).as_tuple().exponent))
            size_precision = abs(int(Decimal(instrument.lotSizeFilter.qtyStep).as_tuple().exponent))
            price_increment = Price.from_str(tick_size)
            size_increment = Quantity.from_str(step_size)
            PyCondition.in_range(float(tick_size), PRICE_MIN, PRICE_MAX, "tick_size")
            PyCondition.in_range(float(step_size), QUANTITY_MIN, QUANTITY_MAX, "step_size")
            max_quantity = Quantity(
                float(instrument.lotSizeFilter.maxOrderQty),
                precision=size_precision,
            )
            min_quantity = Quantity(
                float(instrument.lotSizeFilter.minOrderQty),
                precision=size_precision,
            )
            min_notional = None
            max_price = Price(float(instrument.priceFilter.maxPrice), precision=price_precision)
            min_price = Price(float(instrument.priceFilter.minPrice), precision=price_precision)
            maker_fee = instrument.get_maker_fee()
            taker_fee = instrument.get_taker_fee()
            ts_event = self._clock.timestamp_ns()
            ts_init = self._clock.timestamp_ns()
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
                max_notional=None,
                min_notional=min_notional,
                max_price=max_price,
                min_price=min_price,
                margin_init=Decimal(0.1),
                margin_maint=Decimal(0.1),
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=ts_event,
                ts_init=ts_init,
                info=self._decoder.decode(self._encoder.encode(instrument)),
            )
            self.add_currency(base_currency)
            self.add_currency(quote_currency)
            self.add(instrument=instrument)
            self._log.debug(f"Added instrument {instrument.id}.")
        except ValueError as e:
            if self._log_warnings:
                self._log.warning(f"Unable to parse instrument {instrument.symbol}, {e}.")
