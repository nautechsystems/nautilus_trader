#!/usr/bin/env python3
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
"""
Example: option-chain backtest from a Tardis-backed catalog.

The catalog must already contain option instruments plus per-instrument QuoteTick
and OptionGreeks data, such as data written by the Tardis Machine replay pipeline.

Run with a built v2 extension:
    python examples/backtest/tardis_option_chain.py --catalog-path /path/to/catalog
"""

from __future__ import annotations

import argparse
from dataclasses import dataclass
from decimal import Decimal
from pathlib import Path
from typing import Any
from typing import Self

from nautilus_trader.backtest import BacktestDataConfig  # type: ignore[attr-defined]
from nautilus_trader.backtest import BacktestEngineConfig  # type: ignore[attr-defined]
from nautilus_trader.backtest import BacktestNode  # type: ignore[attr-defined]
from nautilus_trader.backtest import BacktestRunConfig  # type: ignore[attr-defined]
from nautilus_trader.backtest import BacktestVenueConfig  # type: ignore[attr-defined]
from nautilus_trader.core import UUID4
from nautilus_trader.execution import CappedOptionFeeModel  # type: ignore[attr-defined]
from nautilus_trader.execution import TieredNotionalOptionFeeModel  # type: ignore[attr-defined]
from nautilus_trader.model import AccountType  # type: ignore[attr-defined]
from nautilus_trader.model import BookType  # type: ignore[attr-defined]
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType  # type: ignore[attr-defined]
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import LimitOrder  # type: ignore[attr-defined]
from nautilus_trader.model import MarketOrder  # type: ignore[attr-defined]
from nautilus_trader.model import OmsType  # type: ignore[attr-defined]
from nautilus_trader.model import OptionChainSlice  # type: ignore[attr-defined]
from nautilus_trader.model import OptionSeriesId  # type: ignore[attr-defined]
from nautilus_trader.model import OrderSide  # type: ignore[attr-defined]
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrikeRange  # type: ignore[attr-defined]
from nautilus_trader.model import TimeInForce  # type: ignore[attr-defined]
from nautilus_trader.model import TraderId
from nautilus_trader.persistence import ParquetDataCatalog  # type: ignore[attr-defined]
from nautilus_trader.trading import ImportableStrategyConfig  # type: ignore[attr-defined]
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig  # type: ignore[attr-defined]


VENUE = "DERIBIT"


@dataclass(frozen=True)
class OptionMetadata:
    instrument_id: InstrumentId
    underlying: str
    settlement_currency: str
    expiration_ns: int
    strike: Price


@dataclass(frozen=True)
class SeriesSelection:
    series_id: OptionSeriesId
    instrument_ids: list[InstrumentId]
    strikes: list[Price]
    settlement_currency: str


@dataclass(frozen=True)
class SelectedOption:
    instrument_id: InstrumentId
    strike: Price
    quote: Any
    delta: float | None


class OptionChainBacktestConfig(StrategyConfig):
    _CUSTOM_FIELDS = (
        "series_id",
        "selection_mode",
        "target_delta",
        "delta_tolerance",
        "target_strike",
        "trade_size",
        "snapshot_interval_ms",
    )

    def __new__(cls, *args: Any, **kwargs: Any) -> Self:
        for key in cls._CUSTOM_FIELDS:
            kwargs.pop(key, None)
        return super().__new__(cls, *args, **kwargs)

    def __init__(
        self,
        series_id: str,
        selection_mode: str = "delta",
        target_delta: float = 0.25,
        delta_tolerance: float = 0.05,
        target_strike: str | None = None,
        trade_size: str = "1",
        snapshot_interval_ms: int = 1_000,
        **kwargs: Any,
    ) -> None:
        super().__init__()
        self.series_id = series_id
        self.selection_mode = selection_mode
        self.target_delta = target_delta
        self.delta_tolerance = delta_tolerance
        self.target_strike = target_strike
        self.trade_size = trade_size
        self.snapshot_interval_ms = snapshot_interval_ms


class OptionChainBacktest(Strategy):
    def __init__(self, config: OptionChainBacktestConfig) -> None:
        super().__init__(config)
        self._series_id = OptionSeriesId.from_str(config.series_id)
        self._selection_mode = config.selection_mode
        self._target_delta = config.target_delta
        self._delta_tolerance = config.delta_tolerance
        self._target_strike = Price.from_str(config.target_strike) if config.target_strike else None
        self._trade_size = Quantity.from_str(config.trade_size)
        self._snapshot_interval_ms = config.snapshot_interval_ms
        self._orders_submitted = False

    def on_start(self) -> None:
        if self._selection_mode == "strike":
            if self._target_strike is None:
                raise ValueError("target_strike is required when selection_mode is 'strike'")
            strike_range = StrikeRange.fixed([self._target_strike])
        else:
            strike_range = StrikeRange.delta(self._target_delta, self._delta_tolerance)

        self.subscribe_option_chain(
            self._series_id,
            strike_range=strike_range,
            snapshot_interval_ms=self._snapshot_interval_ms,
        )

    def on_option_chain(self, slice: OptionChainSlice) -> None:
        self.log.info(
            f"OPTION_CHAIN | {slice.series_id} | atm={slice.atm_strike} | "
            f"calls={slice.call_count()} puts={slice.put_count()} strikes={slice.strike_count()}",
        )

        if self._orders_submitted:
            return

        selected = self._select_contract(slice)
        if selected is None:
            return

        self.log.info(
            f"Selected option {selected.instrument_id} at strike {selected.strike} "
            f"with delta {selected.delta}",
        )
        self.submit_order(self._maker_order(selected))
        self.submit_order(self._taker_order(selected))
        self._orders_submitted = True

    def on_stop(self) -> None:
        self.unsubscribe_option_chain(self._series_id)

    def _select_contract(self, slice: OptionChainSlice) -> SelectedOption | None:
        if self._selection_mode == "strike":
            return self._select_by_strike(slice)
        return self._select_by_delta(slice) or self._first_quoted_contract(slice)

    def _select_by_delta(self, slice: OptionChainSlice) -> SelectedOption | None:
        best: tuple[float, SelectedOption] | None = None

        for strike in slice.strikes():
            for data in (slice.get_call(strike), slice.get_put(strike)):
                if data is None or data.greeks is None:
                    continue
                distance = abs(abs(data.greeks.delta) - self._target_delta)
                if distance > self._delta_tolerance:
                    continue
                selected = SelectedOption(
                    instrument_id=data.quote.instrument_id,
                    strike=strike,
                    quote=data.quote,
                    delta=data.greeks.delta,
                )

                if best is None or distance < best[0]:
                    best = (distance, selected)

        return best[1] if best is not None else None

    def _select_by_strike(self, slice: OptionChainSlice) -> SelectedOption | None:
        if self._target_strike is None:
            return None

        data = slice.get_call(self._target_strike) or slice.get_put(self._target_strike)
        if data is None:
            return None
        return SelectedOption(
            instrument_id=data.quote.instrument_id,
            strike=self._target_strike,
            quote=data.quote,
            delta=data.greeks.delta if data.greeks is not None else None,
        )

    def _first_quoted_contract(self, slice: OptionChainSlice) -> SelectedOption | None:
        for strike in slice.strikes():
            data = slice.get_call(strike) or slice.get_put(strike)
            if data is not None:
                return SelectedOption(
                    instrument_id=data.quote.instrument_id,
                    strike=strike,
                    quote=data.quote,
                    delta=data.greeks.delta if data.greeks is not None else None,
                )
        return None

    def _maker_order(self, selected: SelectedOption) -> LimitOrder:
        return LimitOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=selected.instrument_id,
            client_order_id=self._client_order_id("M"),
            order_side=OrderSide.BUY,
            quantity=self._trade_size,
            price=selected.quote.bid_price,
            time_in_force=TimeInForce.GTC,
            post_only=True,
            reduce_only=False,
            quote_quantity=False,
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            contingency_type=ContingencyType.NO_CONTINGENCY,
        )

    def _taker_order(self, selected: SelectedOption) -> MarketOrder:
        return MarketOrder(
            trader_id=self.trader_id,
            strategy_id=self.strategy_id,
            instrument_id=selected.instrument_id,
            client_order_id=self._client_order_id("T"),
            order_side=OrderSide.BUY,
            quantity=self._trade_size,
            init_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            time_in_force=TimeInForce.GTC,
            reduce_only=False,
            quote_quantity=False,
            contingency_type=ContingencyType.NO_CONTINGENCY,
        )

    def _client_order_id(self, label: str) -> ClientOrderId:
        return ClientOrderId(f"{self.strategy_id}-{label}-{UUID4()}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--catalog-path", type=Path, default=Path("./catalog"))
    parser.add_argument("--underlying", default="BTC")
    parser.add_argument("--selection", choices=("delta", "strike"), default="delta")
    parser.add_argument("--target-delta", type=float, default=0.25)
    parser.add_argument("--delta-tolerance", type=float, default=0.05)
    parser.add_argument("--target-strike")
    parser.add_argument("--fee-model", choices=("capped", "tiered"), default="capped")
    parser.add_argument("--chunk-size", type=int, default=10_000)
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    options = load_option_metadata(args.catalog_path, args.underlying)
    selection = nearest_series(options)
    target_strike = args.target_strike

    if args.selection == "strike" and target_strike is None:
        target_strike = str(median_strike(selection.strikes))

    venue_config = BacktestVenueConfig(
        name=VENUE,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        book_type=BookType.L1_MBP,
        starting_balances=[starting_balance(selection.settlement_currency)],
        fee_model=option_fee_model(args.fee_model),
    )
    data = [
        BacktestDataConfig(
            data_type="QuoteTick",
            catalog_path=str(args.catalog_path),
            instrument_ids=selection.instrument_ids,
        ),
        BacktestDataConfig(
            data_type="OptionGreeks",
            catalog_path=str(args.catalog_path),
            instrument_ids=selection.instrument_ids,
        ),
    ]
    run_config = BacktestRunConfig(
        id="tardis-option-chain",
        engine=BacktestEngineConfig(
            trader_id=TraderId("OPTION-CHAIN-001"),
            run_analysis=False,
        ),
        venues=[venue_config],
        data=data,
        chunk_size=args.chunk_size,
    )

    module_name = Path(__file__).stem
    strategy_config = ImportableStrategyConfig(
        strategy_path=f"{module_name}:OptionChainBacktest",
        config_path=f"{module_name}:OptionChainBacktestConfig",
        config={
            "series_id": str(selection.series_id),
            "selection_mode": args.selection,
            "target_delta": args.target_delta,
            "delta_tolerance": args.delta_tolerance,
            "target_strike": target_strike,
            "trade_size": "1",
            "snapshot_interval_ms": 1_000,
        },
    )

    node = BacktestNode([run_config])
    node.build()
    node.add_strategy_from_config(run_config.id, strategy_config)
    node.run()
    node.dispose()


def load_option_metadata(catalog_path: Path, underlying: str) -> list[OptionMetadata]:
    catalog = ParquetDataCatalog(str(catalog_path))
    options = [
        metadata
        for instrument in catalog.instruments()
        if (metadata := option_metadata(instrument)) is not None
        and metadata.instrument_id.venue.value == VENUE
        and metadata.underlying.upper() == underlying.upper()
    ]

    if not options:
        raise RuntimeError(
            f"No {underlying} option instruments found for {VENUE} in {catalog_path}",
        )
    return options


def option_metadata(instrument: Any) -> OptionMetadata | None:
    if instrument.type_name == "CryptoOption":
        return OptionMetadata(
            instrument_id=instrument.id,
            underlying=instrument.underlying.code,
            settlement_currency=instrument.settlement_currency.code,
            expiration_ns=instrument.expiration_ns,
            strike=instrument.strike_price,
        )
    if instrument.type_name == "OptionContract":
        return OptionMetadata(
            instrument_id=instrument.id,
            underlying=instrument.underlying,
            settlement_currency=instrument.currency.code,
            expiration_ns=instrument.expiration_ns,
            strike=instrument.strike_price,
        )
    return None


def nearest_series(options: list[OptionMetadata]) -> SeriesSelection:
    expiration_ns = min(metadata.expiration_ns for metadata in options)
    same_expiry = [metadata for metadata in options if metadata.expiration_ns == expiration_ns]
    settlement_currency = next(
        (
            metadata.settlement_currency
            for metadata in same_expiry
            if metadata.settlement_currency == metadata.underlying
        ),
        same_expiry[0].settlement_currency,
    )
    instrument_ids = [
        metadata.instrument_id
        for metadata in same_expiry
        if metadata.settlement_currency == settlement_currency
    ]
    strikes = sorted(
        {
            metadata.strike
            for metadata in same_expiry
            if metadata.settlement_currency == settlement_currency
        },
    )
    series_id = OptionSeriesId(
        VENUE,
        same_expiry[0].underlying,
        settlement_currency,
        expiration_ns,
    )
    return SeriesSelection(series_id, instrument_ids, strikes, settlement_currency)


def median_strike(strikes: list[Price]) -> Price:
    return strikes[len(strikes) // 2]


def option_fee_model(name: str) -> CappedOptionFeeModel | TieredNotionalOptionFeeModel:
    if name == "tiered":
        return TieredNotionalOptionFeeModel(
            maker_rate=Decimal("0.0002"),
            taker_rate=Decimal("0.0005"),
        )
    return CappedOptionFeeModel(
        maker_rate=Decimal("0.0003"),
        taker_rate=Decimal("0.0003"),
    )


def starting_balance(settlement_currency: str) -> str:
    if settlement_currency in {"BTC", "ETH"}:
        return f"10 {settlement_currency}"
    return f"1000000 {settlement_currency}"


if __name__ == "__main__":
    main()
