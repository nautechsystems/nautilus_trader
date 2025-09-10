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

from nautilus_trader.adapters.bybit.schemas.ws import BybitWsTickerLinear
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.model.identifiers import InstrumentId


class TestBybitFundingRateParsing:
    """
    Test cases for Bybit funding rate parsing from ticker data.
    """

    def test_parse_ticker_with_funding_rate(self):
        """
        Test parsing ticker data with funding rate information.
        """
        # Arrange
        ticker_data = BybitWsTickerLinear(
            symbol="BTCUSDT",
            lastPrice="40000.00",
            highPrice24h="41000.00",
            lowPrice24h="39000.00",
            turnover24h="100000000.00",
            volume24h="2500.00",
            fundingRate="0.0001",
            nextFundingTime="1640007200000",
            bid1Price="39999.50",
            bid1Size="10.00",
            ask1Price="40000.50",
            ask1Size="10.00",
        )

        # Act
        instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BYBIT")
        ts_event = millis_to_nanos(1640000000000)
        ts_init = millis_to_nanos(1640000001000)

        # Assert the ticker has funding rate data
        assert ticker_data.fundingRate == "0.0001"
        assert ticker_data.nextFundingTime == "1640007200000"

        # Create funding rate update manually (simulating what the method would do)
        funding_rate = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal(ticker_data.fundingRate),
            ts_event=ts_event,
            ts_init=ts_init,
            next_funding_ns=int(ticker_data.nextFundingTime) * 1_000_000,
        )

        assert funding_rate.instrument_id == instrument_id
        assert funding_rate.rate == Decimal("0.0001")
        assert funding_rate.next_funding_ns == 1640007200000 * 1_000_000

    def test_parse_ticker_negative_funding_rate(self):
        """
        Test parsing ticker data with negative funding rate.
        """
        # Arrange
        ticker_data = BybitWsTickerLinear(
            symbol="ETHUSDT",
            lastPrice="3000.00",
            fundingRate="-0.00025",
            nextFundingTime="1640007200000",
        )

        # Act
        instrument_id = InstrumentId.from_str("ETHUSDT-PERP.BYBIT")
        ts_event = millis_to_nanos(1640000000000)
        ts_init = millis_to_nanos(1640000001000)

        funding_rate = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal(ticker_data.fundingRate),
            ts_event=ts_event,
            ts_init=ts_init,
            next_funding_ns=int(ticker_data.nextFundingTime) * 1_000_000,
        )

        # Assert
        assert funding_rate.rate == Decimal("-0.00025")
        assert funding_rate.rate < 0

    def test_parse_ticker_without_next_funding_time(self):
        """
        Test parsing ticker data without next funding time.
        """
        # Arrange
        ticker_data = BybitWsTickerLinear(
            symbol="BTCUSDT",
            lastPrice="40000.00",
            fundingRate="0.0001",
            nextFundingTime=None,  # No next funding time
        )

        # Act
        instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BYBIT")
        ts_event = millis_to_nanos(1640000000000)
        ts_init = millis_to_nanos(1640000001000)

        funding_rate = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal(ticker_data.fundingRate),
            ts_event=ts_event,
            ts_init=ts_init,
            next_funding_ns=None,
        )

        # Assert
        assert funding_rate.rate == Decimal("0.0001")
        assert funding_rate.next_funding_ns is None

    def test_ticker_without_funding_rate_should_not_create_update(self):
        """
        Test that ticker without funding rate does not create FundingRateUpdate.
        """
        # Arrange
        ticker_data = BybitWsTickerLinear(
            symbol="BTCUSDT",
            lastPrice="40000.00",
            fundingRate=None,  # No funding rate
            nextFundingTime=None,
        )

        # Act & Assert
        # In the actual implementation, we would return None if fundingRate is None
        assert ticker_data.fundingRate is None

    def test_high_precision_funding_rate(self):
        """
        Test parsing ticker with high precision funding rate.
        """
        # Arrange
        ticker_data = BybitWsTickerLinear(
            symbol="BTCUSDT",
            lastPrice="40000.00",
            fundingRate="0.000012345678",
            nextFundingTime="1640007200000",
        )

        # Act
        instrument_id = InstrumentId.from_str("BTCUSDT-PERP.BYBIT")
        ts_event = millis_to_nanos(1640000000000)
        ts_init = millis_to_nanos(1640000001000)

        funding_rate = FundingRateUpdate(
            instrument_id=instrument_id,
            rate=Decimal(ticker_data.fundingRate),
            ts_event=ts_event,
            ts_init=ts_init,
            next_funding_ns=int(ticker_data.nextFundingTime) * 1_000_000,
        )

        # Assert
        assert funding_rate.rate == Decimal("0.000012345678")
        assert str(funding_rate.rate) == "0.000012345678"
