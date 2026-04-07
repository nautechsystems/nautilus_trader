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

import pytest

from nautilus_trader.adapters.adanos import AdanosSentimentSnapshot
from nautilus_trader.adapters.adanos import adanos_sentiment_data_type
from nautilus_trader.adapters.adanos import build_adanos_sentiment_snapshot
from nautilus_trader.adapters.adanos import wrap_adanos_sentiment_snapshot
from nautilus_trader.model.data import CustomData
from nautilus_trader.persistence.catalog import ParquetDataCatalog
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def test_build_adanos_sentiment_snapshot_computes_aggregate_fields() -> None:
    instrument = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")

    snapshot = build_adanos_sentiment_snapshot(
        instrument.id,
        ts_event=1,
        reddit={"buzz_score": 70.0, "bullish_pct": 72, "mentions": 90, "company_name": "Apple Inc."},
        x={"buzz_score": 60.0, "bullish_pct": 64, "mentions": 500},
        news={"buzz_score": 40.0, "bullish_pct": 35, "mentions": 15},
        polymarket={"buzz_score": 50.0, "bullish_pct": 61, "trade_count": 75, "market_count": 2},
    )

    assert snapshot.instrument_id == instrument.id
    assert snapshot.symbol == "AAPL"
    assert snapshot.company_name == "Apple Inc."
    assert snapshot.coverage == 4
    assert snapshot.average_buzz == pytest.approx(55.0)
    assert snapshot.average_bullish_pct == pytest.approx(58.0)
    assert snapshot.source_alignment == "divergent"
    assert snapshot.alignment_score == pytest.approx(0.0)
    assert snapshot.polymarket_trade_count == 75

    wrapped = wrap_adanos_sentiment_snapshot(snapshot)
    assert isinstance(wrapped, CustomData)
    assert wrapped.data_type == adanos_sentiment_data_type(instrument.id)
    assert wrapped.data_type.metadata == {
        "instrument_id": instrument.id.value,
        "vendor": "adanos",
    }


def test_adanos_sentiment_snapshot_roundtrips_via_catalog(tmp_path) -> None:
    instrument = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir(parents=True, exist_ok=True)
    catalog = ParquetDataCatalog(str(catalog_path))

    snapshot = build_adanos_sentiment_snapshot(
        instrument.id,
        ts_event=1_000,
        ts_init=1_500,
        reddit={"buzz_score": 64.0, "bullish_pct": 69, "mentions": 80, "company_name": "Apple Inc."},
        x={"buzz_score": 58.0, "bullish_pct": 60, "mentions": 320},
        news={"buzz_score": 54.0, "bullish_pct": 57, "mentions": 12},
    )
    wrapped = wrap_adanos_sentiment_snapshot(snapshot)

    catalog.write_data([wrapped])
    restored_rows = catalog.query(AdanosSentimentSnapshot, identifiers=[instrument.id.value])

    assert len(restored_rows) == 1
    restored = restored_rows[0]
    assert isinstance(restored, CustomData)
    assert isinstance(restored.data, AdanosSentimentSnapshot)
    assert restored.data.instrument_id == instrument.id
    assert restored.data.average_buzz == pytest.approx(snapshot.average_buzz)
    assert restored.data.average_bullish_pct == pytest.approx(snapshot.average_bullish_pct)
    assert restored.data.source_alignment == "aligned"


def test_adanos_sentiment_snapshot_uses_sentiment_score_fallback() -> None:
    instrument = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")

    snapshot = build_adanos_sentiment_snapshot(
        instrument.id,
        ts_event=1,
        reddit={"buzz_score": 50.0, "sentiment_score": 0.5, "mentions": 20},
    )

    assert snapshot.reddit_bullish_pct == pytest.approx(75.0)
    assert snapshot.average_bullish_pct == pytest.approx(75.0)
    assert snapshot.source_alignment == "single_source"
    assert snapshot.alignment_score == pytest.approx(0.25)


def test_adanos_sentiment_snapshot_serializes_roundtrip() -> None:
    instrument = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")

    snapshot = build_adanos_sentiment_snapshot(
        instrument.id,
        ts_event=1_000,
        ts_init=1_500,
        company_name="Apple Inc.",
        reddit={"buzz_score": 61.0, "bullish_pct": 68, "mentions": 50},
        x={"buzz_score": 54.0, "bullish_pct": 58, "mentions": 220},
    )

    assert AdanosSentimentSnapshot.from_dict(snapshot.to_dict()) == snapshot
    assert AdanosSentimentSnapshot.from_bytes(snapshot.to_bytes()) == snapshot
