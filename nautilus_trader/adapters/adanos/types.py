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

from collections.abc import Mapping
from math import isnan
from typing import Any

from nautilus_trader.core.data import Data
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.identifiers import InstrumentId


_ADANOS_VENDOR = "adanos"


@customdataclass
class AdanosSentimentSnapshot(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("AAPL.XNAS")
    symbol: str = ""
    company_name: str = ""
    source_alignment: str = "unavailable"
    alignment_score: float = 0.0
    coverage: int = 0
    average_buzz: float = 0.0
    average_bullish_pct: float = 0.0
    reddit_buzz: float = 0.0
    reddit_bullish_pct: float = 0.0
    reddit_mentions: int = 0
    x_buzz: float = 0.0
    x_bullish_pct: float = 0.0
    x_mentions: int = 0
    news_buzz: float = 0.0
    news_bullish_pct: float = 0.0
    news_mentions: int = 0
    polymarket_buzz: float = 0.0
    polymarket_bullish_pct: float = 0.0
    polymarket_trade_count: int = 0
    polymarket_market_count: int = 0


def adanos_sentiment_metadata(instrument_id: InstrumentId) -> dict[str, str]:
    return {
        "instrument_id": instrument_id.value,
        "vendor": _ADANOS_VENDOR,
    }


def adanos_sentiment_data_type(instrument_id: InstrumentId) -> DataType:
    return DataType(
        AdanosSentimentSnapshot,
        metadata=adanos_sentiment_metadata(instrument_id),
    )


def build_adanos_sentiment_snapshot(
    instrument_id: InstrumentId,
    *,
    ts_event: int,
    ts_init: int | None = None,
    company_name: str = "",
    reddit: Mapping[str, Any] | None = None,
    x: Mapping[str, Any] | None = None,
    news: Mapping[str, Any] | None = None,
    polymarket: Mapping[str, Any] | None = None,
) -> AdanosSentimentSnapshot:
    ts_init = ts_event if ts_init is None else ts_init

    reddit_buzz = _extract_float(reddit, "buzz_score")
    reddit_bullish_pct = _extract_bullish_pct(reddit)
    reddit_mentions = _extract_int(reddit, "mentions")

    x_buzz = _extract_float(x, "buzz_score")
    x_bullish_pct = _extract_bullish_pct(x)
    x_mentions = _extract_int(x, "mentions")

    news_buzz = _extract_float(news, "buzz_score")
    news_bullish_pct = _extract_bullish_pct(news)
    news_mentions = _extract_int(news, "mentions")

    polymarket_buzz = _extract_float(polymarket, "buzz_score")
    polymarket_bullish_pct = _extract_bullish_pct(polymarket)
    polymarket_trade_count = _extract_int(polymarket, "trade_count")
    polymarket_market_count = _extract_int(polymarket, "market_count")

    buzz_values = [
        value
        for value in (reddit_buzz, x_buzz, news_buzz, polymarket_buzz)
        if value > 0.0
    ]
    bullish_values = [
        value
        for value in (
            reddit_bullish_pct,
            x_bullish_pct,
            news_bullish_pct,
            polymarket_bullish_pct,
        )
        if value > 0.0
    ]

    coverage = sum(
        1
        for source in (reddit, x, news, polymarket)
        if _source_has_signal(source)
    )
    source_alignment, alignment_score = _classify_alignment(bullish_values)

    return AdanosSentimentSnapshot(
        instrument_id=instrument_id,
        symbol=instrument_id.symbol.value,
        company_name=company_name
        or _first_non_empty(reddit, x, news, polymarket, key="company_name"),
        source_alignment=source_alignment,
        alignment_score=alignment_score,
        coverage=coverage,
        average_buzz=_average(buzz_values),
        average_bullish_pct=_average(bullish_values),
        reddit_buzz=reddit_buzz,
        reddit_bullish_pct=reddit_bullish_pct,
        reddit_mentions=reddit_mentions,
        x_buzz=x_buzz,
        x_bullish_pct=x_bullish_pct,
        x_mentions=x_mentions,
        news_buzz=news_buzz,
        news_bullish_pct=news_bullish_pct,
        news_mentions=news_mentions,
        polymarket_buzz=polymarket_buzz,
        polymarket_bullish_pct=polymarket_bullish_pct,
        polymarket_trade_count=polymarket_trade_count,
        polymarket_market_count=polymarket_market_count,
        ts_event=ts_event,
        ts_init=ts_init,
    )


def wrap_adanos_sentiment_snapshot(snapshot: AdanosSentimentSnapshot) -> CustomData:
    return CustomData(adanos_sentiment_data_type(snapshot.instrument_id), snapshot)


def _extract_float(source: Mapping[str, Any] | None, key: str) -> float:
    if not source:
        return 0.0

    value = source.get(key)
    if value is None:
        return 0.0

    try:
        number = float(value)
    except (TypeError, ValueError):
        return 0.0

    if isnan(number):
        return 0.0

    return number


def _extract_int(source: Mapping[str, Any] | None, key: str) -> int:
    if not source:
        return 0

    value = source.get(key)
    if value is None:
        return 0

    try:
        return int(value)
    except (TypeError, ValueError):
        return 0


def _extract_bullish_pct(source: Mapping[str, Any] | None) -> float:
    bullish_pct = _extract_float(source, "bullish_pct")
    if bullish_pct > 0.0:
        return bullish_pct

    sentiment_score = _extract_float(source, "sentiment_score")
    if sentiment_score == 0.0:
        return 0.0

    return max(0.0, min(100.0, ((sentiment_score + 1.0) / 2.0) * 100.0))


def _source_has_signal(source: Mapping[str, Any] | None) -> bool:
    if not source:
        return False

    return any(
        [
            _extract_float(source, "buzz_score") > 0.0,
            _extract_float(source, "bullish_pct") > 0.0,
            _extract_float(source, "sentiment_score") != 0.0,
            _extract_int(source, "mentions") > 0,
            _extract_int(source, "trade_count") > 0,
            _extract_int(source, "market_count") > 0,
        ],
    )


def _classify_alignment(bullish_values: list[float]) -> tuple[str, float]:
    if not bullish_values:
        return "unavailable", 0.0

    directions = []
    for bullish_pct in bullish_values:
        if bullish_pct >= 60.0:
            directions.append("bullish")
        elif bullish_pct <= 40.0:
            directions.append("bearish")
        else:
            directions.append("neutral")

    directional = [direction for direction in directions if direction != "neutral"]
    unique_directional = set(directional)

    if len(bullish_values) == 1:
        return "single_source", 0.25
    if len(unique_directional) == 1 and directional:
        return "aligned", 1.0
    if len(unique_directional) > 1:
        return "divergent", 0.0
    return "mixed", 0.5


def _average(values: list[float]) -> float:
    if not values:
        return 0.0

    return sum(values) / len(values)


def _first_non_empty(*sources: Mapping[str, Any] | None, key: str) -> str:
    for source in sources:
        if not source:
            continue
        value = source.get(key)
        if isinstance(value, str) and value:
            return value
    return ""
