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

from abc import abstractmethod
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXContractType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentStatus
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.symbol import OKXSymbol
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import QUANTITY_MAX
from nautilus_trader.model.objects import QUANTITY_MIN
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class OKXInstrumentBase(msgspec.Struct):
    instType: OKXInstrumentType
    instId: str
    uly: str
    instFamily: str
    category: str  # deprecated
    baseCcy: str
    quoteCcy: str
    settleCcy: str
    ctVal: str
    ctMult: str
    ctValCcy: str
    optType: str
    stk: str
    listTime: str
    expTime: str
    lever: str
    tickSz: str
    lotSz: str
    minSz: str
    ctType: OKXContractType
    alias: str
    state: OKXInstrumentStatus
    ruleType: str
    maxLmtSz: str
    maxMktSz: str
    maxLmtAmt: str
    maxMktAmt: str
    maxTwapSz: str
    maxIcebergSz: str
    maxTriggerSz: str
    maxStopSz: str

    @abstractmethod
    def parse_to_instrument(
        self,
        base_currency: Currency,
        quote_currency: Currency,
        maker_fee: str | Decimal,
        taker_fee: str | Decimal,
        margin_init: str | Decimal,
        margin_maint: str | Decimal,
        ts_event: int,
        ts_init: int,
    ) -> CryptoPerpetual | CurrencyPair | CryptoFuture | OptionContract:
        pass

    def _clip_qty(self, value: str) -> float:
        return max(min(float(value), QUANTITY_MAX), QUANTITY_MIN)

    def _clip_prc(self, value: str) -> float:
        return max(float(value), 1e-9)


class OKXInstrumentSpot(OKXInstrumentBase):
    def parse_to_instrument(
        self,
        base_currency: Currency,
        quote_currency: Currency,
        maker_fee: str | Decimal,
        taker_fee: str | Decimal,
        margin_init: str | Decimal,
        margin_maint: str | Decimal,
        ts_event: int,
        ts_init: int,
    ) -> CurrencyPair:
        assert self.instType in [OKXInstrumentType.SPOT, OKXInstrumentType.MARGIN]
        assert base_currency.code == self.baseCcy
        assert quote_currency.code == self.quoteCcy

        # NOTE: truncate all float strings to precision 9 (max in nautilus)
        # NOTE: can use instrument.info (dict[str, Any]) to get raw API data

        # TODO: Fix for correct precisions
        price_increment = Price.from_str(f"{self._clip_prc(self.tickSz):.9f}")
        size_increment = Quantity.from_str(f"{self._clip_qty(self.lotSz):.9f}")

        # Get max_quantity as the min of all possible max sizes
        max_quantity = None
        if any([self.maxLmtSz, self.maxMktSz, self.maxTwapSz, self.maxTriggerSz, self.maxStopSz]):
            max_quantity = min(
                [
                    Quantity.from_str(f"{self._clip_qty(q):.9f}")
                    for q in [
                        self.maxLmtSz,
                        self.maxMktSz,
                        self.maxTwapSz,
                        self.maxTriggerSz,
                        self.maxStopSz,
                        # self.maxIcebergSz, # probably not relevant
                    ]
                    if q != ""
                ],
            )
        min_quantity = (
            Quantity.from_str(f"{self._clip_qty(self.minSz):.9f}") if self.minSz else None
        )

        # Get max_notional as the min of all possible max USD amounts
        max_notional = None
        if any([self.maxLmtAmt, self.maxMktAmt]):
            max_notional = min(
                [
                    Money.from_str(f"{float(q):.2f} USD")
                    for q in [self.maxLmtAmt, self.maxMktAmt]
                    if q != ""
                ],
            )

        okx_symbol = OKXSymbol.from_raw_symbol(self.instId, self.instType, self.ctType)

        maker_fee = -Decimal(maker_fee)  # NOTE for OKX, positive means rebate, need to flip sign
        taker_fee = -Decimal(taker_fee)  # NOTE for OKX, positive means rebate, need to flip sign

        instrument = CurrencyPair(
            instrument_id=okx_symbol.to_instrument_id(),
            raw_symbol=Symbol(okx_symbol.raw_symbol),
            base_currency=base_currency,
            quote_currency=quote_currency,
            price_precision=price_increment.precision,
            size_precision=size_increment.precision,
            price_increment=price_increment,
            size_increment=size_increment,
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            max_notional=max_notional,
            min_notional=None,
            max_price=None,
            min_price=None,
            lot_size=size_increment,
            margin_init=Decimal(margin_init),
            margin_maint=Decimal(margin_maint),
            maker_fee=round(maker_fee, 6),
            taker_fee=round(taker_fee, 6),
            ts_event=ts_event,
            ts_init=ts_init,
            info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
        )

        return instrument


class OKXInstrumentSwap(OKXInstrumentBase):
    def parse_to_instrument(
        self,
        base_currency: Currency,
        quote_currency: Currency,
        maker_fee: str | Decimal,
        taker_fee: str | Decimal,
        margin_init: str | Decimal,
        margin_maint: str | Decimal,
        ts_event: int,
        ts_init: int,
    ) -> CryptoPerpetual:
        assert self.instType == OKXInstrumentType.SWAP
        assert base_currency.code == self.ctValCcy
        assert quote_currency.code == self.settleCcy

        # NOTE: truncate all float strings to precision 9 (max in nautilus)
        # NOTE: can use instrument.info (dict[str, Any]) to get raw API data

        # TODO: Fix for correct precisions
        price_increment = Price.from_str(f"{self._clip_prc(self.tickSz):.9f}")
        size_increment = Quantity.from_str(f"{self._clip_qty(self.lotSz):.9f}")

        # Get max_quantity as the min of all possible max sizes
        max_quantity = None
        if any([self.maxLmtSz, self.maxMktSz, self.maxTwapSz, self.maxTriggerSz, self.maxStopSz]):
            max_quantity = min(
                [
                    Quantity.from_str(f"{self._clip_qty(q):.9f}")
                    for q in [
                        self.maxLmtSz,
                        self.maxMktSz,
                        self.maxTwapSz,
                        self.maxTriggerSz,
                        self.maxStopSz,
                        # self.maxIcebergSz, # probably not relevant
                    ]
                    if q != ""
                ],
            )
        min_quantity = (
            Quantity.from_str(f"{self._clip_qty(self.minSz):.9f}") if self.minSz else None
        )

        # Get max_notional as the min of all possible max USD amounts
        max_notional = None
        if any([self.maxLmtAmt, self.maxMktAmt]):
            max_notional = min(
                [
                    Money.from_str(f"{float(q):.2f} USD")
                    for q in [self.maxLmtAmt, self.maxMktAmt]
                    if q != ""
                ],
            )

        maker_fee = -Decimal(maker_fee)  # NOTE for OKX, positive means rebate, need to flip sign
        taker_fee = -Decimal(taker_fee)  # NOTE for OKX, positive means rebate, need to flip sign

        okx_symbol = OKXSymbol.from_raw_symbol(self.instId, self.instType, self.ctType)
        instrument = CryptoPerpetual(
            instrument_id=okx_symbol.to_instrument_id(),
            raw_symbol=Symbol(okx_symbol.raw_symbol),
            base_currency=base_currency,
            quote_currency=quote_currency,
            settlement_currency=quote_currency,
            is_inverse=self.ctType == OKXContractType.INVERSE,
            price_precision=price_increment.precision,
            size_precision=size_increment.precision,
            price_increment=price_increment,
            size_increment=size_increment,
            multiplier=Quantity.from_str(self.ctVal),
            max_quantity=max_quantity,
            min_quantity=min_quantity,
            max_notional=max_notional,
            min_notional=None,
            max_price=None,
            min_price=None,
            margin_init=Decimal(margin_init),
            margin_maint=Decimal(margin_maint),
            maker_fee=round(maker_fee, 6),
            taker_fee=round(taker_fee, 6),
            ts_event=ts_event,
            ts_init=ts_init,
            info=msgspec.json.Decoder().decode(msgspec.json.Encoder().encode(self)),
        )

        return instrument


OKXInstrument = OKXInstrumentSpot | OKXInstrumentSwap

OKXInstrumentList = list[OKXInstrumentSpot] | list[OKXInstrumentSwap]


class OKXInstrumentsSpotResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXInstrumentSpot]


class OKXInstrumentsSwapResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXInstrumentSwap]
