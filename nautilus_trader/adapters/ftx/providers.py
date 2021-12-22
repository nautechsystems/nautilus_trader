# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import time
from datetime import datetime
from decimal import Decimal
from typing import Any, Dict, List

from nautilus_trader.adapters.ftx.common import FTX_VENUE
from nautilus_trader.adapters.ftx.http.client import FTXHttpClient
from nautilus_trader.adapters.ftx.http.error import FTXClientError
from nautilus_trader.common.logging import Logger
from nautilus_trader.common.logging import LoggerAdapter
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.text import precision_from_str
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments.crypto_perp import CryptoPerpetual
from nautilus_trader.model.instruments.currency import CurrencySpot
from nautilus_trader.model.instruments.future import Future
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class FTXInstrumentProvider(InstrumentProvider):
    """
    Provides a means of loading `Instrument` from the FTX API.

    Parameters
    ----------
    client : APIClient
        The client for the provider.
    logger : Logger
        The logger for the provider.
    """

    def __init__(
        self,
        client: FTXHttpClient,
        logger: Logger,
    ):
        super().__init__()

        self.venue = FTX_VENUE
        self._client = client
        self._log = LoggerAdapter(type(self).__name__, logger)

        # Async loading flags
        self._loaded = False
        self._loading = False

    async def load_all_or_wait_async(self) -> None:
        """
        Load the latest FTX instruments into the provider asynchronously, or
        await loading.

        If `load_async` has been previously called then will immediately return.
        """
        if self._loaded:
            return  # Already loaded

        if not self._loading:
            self._log.debug("Loading instruments...")
            await self.load_all_async()
            self._log.info(f"Loaded {self.count} instruments.")
        else:
            self._log.debug("Awaiting loading...")
            while self._loading:
                # Wait 100ms
                await asyncio.sleep(0.1)

    async def load_all_async(self) -> None:  # noqa  # TODO(cs): WIP
        """
        Load the latest FTX instruments into the provider asynchronously.

        """
        # Set async loading flag
        self._loading = True

        # Get current commission rates
        try:
            account_info: Dict[str, Any] = await self._client.get_account_info()
        except FTXClientError:
            self._log.error(
                "Cannot load instruments: API key authentication failed "
                "(this is needed to fetch the applicable account fee tier).",
            )
            return

        assets_res: List[Dict[str, Any]] = await self._client.list_markets()

        for info in assets_res:
            native_symbol = Symbol(info["name"])

            asset_type = info["type"]

            # Create base asset
            if asset_type == "future":
                base_asset: str = info["underlying"]
                base_currency = Currency(
                    code=base_asset,
                    precision=8,
                    iso4217=0,  # Currently undetermined for crypto assets
                    name=base_asset,
                    currency_type=CurrencyType.CRYPTO,
                )

                quote_currency: Currency = USD
            elif asset_type == "spot":
                base_asset = info["baseCurrency"]
                base_currency = Currency(
                    code=base_asset,
                    precision=8,
                    iso4217=0,  # Currently undetermined for crypto assets
                    name=base_asset,
                    currency_type=CurrencyType.CRYPTO,
                )
                if not info.get("tokenizedEquity"):
                    self.add_currency(currency=base_currency)

                # Create quote asset
                quote_asset: str = info["quoteCurrency"]
                quote_currency = Currency.from_str(quote_asset)
                if quote_currency is None:
                    quote_currency = Currency(
                        code=quote_asset,
                        precision=precision_from_str(str(info["priceIncrement"])),
                        iso4217=0,  # Currently undetermined for crypto assets
                        name=quote_asset,
                        currency_type=CurrencyType.CRYPTO,
                    )
            else:  # pragma: no cover (design-time error)
                raise RuntimeError(f"unknown asset type, was {asset_type}")

            # symbol = Symbol(base_currency.code + "/" + quote_currency.code)
            instrument_id = InstrumentId(symbol=native_symbol, venue=FTX_VENUE)

            price_precision = precision_from_str(str(info["priceIncrement"]))
            size_precision = precision_from_str(str(info["sizeIncrement"]))
            price_increment = Price.from_str(str(info["priceIncrement"]))
            size_increment = Quantity.from_str(str(info["sizeIncrement"]))
            lot_size = Quantity.from_str(str(info["minProvideSize"]))
            margin_init = Decimal(str(account_info["initialMarginRequirement"]))
            margin_maint = Decimal(str(account_info["maintenanceMarginRequirement"]))
            maker_fee = Decimal(str(account_info.get("makerFee")))
            taker_fee = Decimal(str(account_info.get("takerFee")))

            if asset_type == "spot":
                # Create instrument
                instrument = CurrencySpot(
                    instrument_id=instrument_id,
                    local_symbol=native_symbol,
                    base_currency=base_currency,
                    quote_currency=quote_currency,
                    price_precision=price_precision,
                    size_precision=size_precision,
                    price_increment=price_increment,
                    size_increment=size_increment,
                    lot_size=lot_size,
                    max_quantity=None,
                    min_quantity=None,
                    max_notional=None,
                    min_notional=None,
                    max_price=None,
                    min_price=None,
                    margin_init=margin_init,
                    margin_maint=margin_maint,
                    maker_fee=maker_fee,
                    taker_fee=taker_fee,
                    ts_event=time.time_ns(),
                    ts_init=time.time_ns(),
                    info=info,
                )
            elif asset_type == "future":
                # Create instrument
                if info["name"].endswith("-PERP"):
                    instrument = CryptoPerpetual(
                        instrument_id=instrument_id,
                        local_symbol=native_symbol,
                        base_currency=base_currency,
                        quote_currency=quote_currency,
                        settlement_currency=USD,
                        is_inverse=False,
                        price_precision=price_precision,
                        size_precision=size_precision,
                        price_increment=price_increment,
                        size_increment=size_increment,
                        max_quantity=None,
                        min_quantity=None,
                        max_notional=None,
                        min_notional=None,
                        max_price=None,
                        min_price=None,
                        margin_init=margin_init,
                        margin_maint=margin_maint,
                        maker_fee=maker_fee,
                        taker_fee=taker_fee,
                        ts_event=time.time_ns(),
                        ts_init=time.time_ns(),
                        info=info,
                    )
                else:
                    instrument = Future(
                        instrument_id=instrument_id,
                        local_symbol=native_symbol,
                        asset_class=AssetClass.CRYPTO,
                        currency=USD,
                        price_precision=price_precision,
                        price_increment=price_increment,
                        multiplier=Quantity.from_int(1),
                        lot_size=Quantity.from_int(1),
                        underlying=info["underlying"],
                        expiry_date=datetime.utcnow().date(),  # TODO(cs): Implement
                        # margin_init=margin_init,  # TODO(cs): Implement
                        # margin_maint=margin_maint,  # TODO(cs): Implement
                        # maker_fee=maker_fee,  # TODO(cs): Implement
                        # taker_fee=taker_fee,  # TODO(cs): Implement
                        ts_event=time.time_ns(),
                        ts_init=time.time_ns(),
                    )

            self.add_currency(currency=quote_currency)
            self.add(instrument=instrument)

        # Set async loading flags
        self._loading = False
        self._loaded = True
