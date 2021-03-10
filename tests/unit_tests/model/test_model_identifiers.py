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

import pytest

from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import AssetType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import Exchange
from nautilus_trader.model.identifiers import FutureSecurity
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import Identifier
from nautilus_trader.model.identifiers import Issuer
from nautilus_trader.model.identifiers import OrderId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import Security
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue


class TestIdentifiers:

    @pytest.mark.parametrize(
        "value, ex",
        [[None, TypeError],
         ["", ValueError],
         [" ", ValueError],
         ["  ", ValueError],
         [1234, TypeError]],
    )
    def test_instantiate_given_various_invalid_values_raises_exception(self, value, ex):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ex):
            Identifier(value)

    def test_equality(self):
        # Arrange
        id1 = Identifier("abc123")
        id2 = Identifier("abc123")
        id3 = Identifier("def456")

        # Act
        # Assert
        assert "abc123" == id1.value
        assert id1 == id1
        assert id1 == id2
        assert id1 != id3

    def test_equality_of_subclass(self):
        # Arrange
        id1 = Exchange("BINANCE")
        id2 = Exchange("BINANCE")
        id3 = Security(Symbol("BINANCE"), Exchange("BINANCE"), AssetClass.CRYPTO, AssetType.SPOT)  # Invalid
        id4 = IdTag("BINANCE")

        # Act
        # Assert
        assert id1 == id1
        assert id2 == id2
        assert id1 == id2
        assert id2 == id1
        assert id1 != id3
        assert id2 != id3
        assert id2 != id4
        assert id4 != id1

    def test_comparison(self):
        # Arrange
        string1 = Identifier("123")
        string2 = Identifier("456")
        string3 = Identifier("abc")
        string4 = Identifier("def")

        # Act
        # Assert
        assert string1 <= string1
        assert string1 <= string2
        assert string1 < string2
        assert string2 > string1
        assert string2 >= string1
        assert string2 >= string2
        assert string3 <= string4

    def test_hash(self):
        # Arrange
        identifier1 = Identifier("abc")
        identifier2 = Identifier("abc")

        # Act
        # Assert
        assert isinstance(hash(identifier1), int)
        assert hash(identifier1) == hash(identifier2)

    def test_identifier_equality(self):
        # Arrange
        id1 = Identifier("some-id-1")
        id2 = Identifier("some-id-2")

        # Act
        # Assert
        assert id1 == id1
        assert id1 != id2

    def test_identifier_to_str(self):
        # Arrange
        identifier = Identifier("some-id")

        # Act
        result = str(identifier)

        # Assert
        assert "some-id" == result

    def test_identifier_repr(self):
        # Arrange
        identifier = Identifier("some-id")

        # Act
        result = repr(identifier)

        # Assert
        assert "Identifier('some-id')" == result

    def test_mixed_identifier_equality(self):
        # Arrange
        id1 = ClientOrderId("O-123456")
        id2 = PositionId("P-123456")

        # Act
        # Assert
        assert id1 == id1
        assert id1 != id2

    def test_account_id_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            AccountId.from_str("BAD_STRING")

    def test_strategy_id_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            StrategyId.from_str("BAD_STRING")

    def test_trader_id_given_malformed_string_raises_value_error(self):
        # Arrange
        # Act
        # Assert
        with pytest.raises(ValueError):
            TraderId.from_str("BAD_STRING")

    def test_trader_identifier(self):
        # Arrange
        # Act
        trader_id1 = TraderId("TESTER", "000")
        trader_id2 = TraderId("TESTER", "001")

        # Assert
        assert trader_id1 == trader_id1
        assert trader_id1 != trader_id2
        assert "TESTER-000" == trader_id1.value
        assert "TESTER" == trader_id1.name
        assert trader_id1 == TraderId.from_str("TESTER-000")

    def test_strategy_identifier(self):
        # Arrange
        # Act
        strategy_id1 = StrategyId.null()
        strategy_id2 = StrategyId("SCALPER", "01")

        # Assert
        assert "NULL-NULL" == strategy_id1.value
        assert strategy_id1 == strategy_id1
        assert strategy_id1 != strategy_id2
        assert "NULL" == strategy_id1.name
        assert strategy_id2 == StrategyId.from_str('SCALPER-01')

    def test_account_identifier(self):
        # Arrange
        # Act
        account_id1 = AccountId("SIM", "02851908")
        account_id2 = AccountId("SIM", "09999999")

        # Assert
        assert account_id1 == account_id1
        assert account_id1 != account_id2
        assert "SIM-02851908", account_id1.value
        assert Issuer("SIM") == account_id1.issuer
        assert account_id1 == AccountId("SIM", "02851908")

    def test_position_identifier(self):
        # Arrange
        # Act
        position_id0 = PositionId.null()

        # Assert
        assert "NULL" == position_id0.value

    def test_order_identifier(self):
        # Arrange
        # Act
        order_id = OrderId.null()

        # Assert
        assert "NULL" == order_id.value


class TestSecurityIdentifier:

    def test_security_equality(self):
        # Arrange
        security1 = Security(Symbol("AUD/USD"), Venue("SIM"), AssetClass.FX, AssetType.SPOT)
        security2 = Security(Symbol("AUD/USD"), Venue("IDEALPRO"), AssetClass.FX, AssetType.SPOT)
        security3 = Security(Symbol("GBP/USD"), Venue("SIM"), AssetClass.FX, AssetType.SPOT)

        # Act
        # Assert
        assert security1 == security1
        assert security1 != security2
        assert security1 != security3

    def test_security_str(self):
        # Arrange
        security = Security(Symbol("AUD/USD"), Venue("SIM"), AssetClass.FX, AssetType.SPOT)

        # Act
        # Assert
        assert "AUD/USD.SIM" == str(security)

    def test_security_repr(self):
        # Arrange
        security = Security(Symbol("AUD/USD"), Venue("SIM"), AssetClass.FX, AssetType.SPOT)

        # Act
        # Assert
        assert "Security('AUD/USD.SIM,FX,SPOT')" == repr(security)

    def test_parse_security_from_str(self):
        # Arrange
        security = Security(Symbol("AUD/USD"), Venue("SIM"), AssetClass.FX, AssetType.SPOT)

        # Act
        result = Security.from_serializable_str(security.to_serializable_str())

        # Assert
        assert security == result


class TestFutureSecurityIdentifier:

    def test_future_security_instantiation(self):
        # Arrange
        security = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
            expiry="201609",
            currency=Currency.from_str("EUR"),
            multiplier=5,
        )

        # Act
        # Assert
        assert Symbol("DAX") == security.symbol
        assert Exchange("DTB") == security.venue
        assert AssetClass.INDEX == security.asset_class
        assert AssetType.FUTURE == security.asset_type
        assert "201609" == security.expiry
        assert Currency.from_str("EUR") == security.currency
        assert 5 == security.multiplier

    def test_future_security_str(self):
        # Arrange
        security = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
            expiry="201609",
            currency=Currency.from_str("EUR"),
            multiplier=5,
        )

        # Act
        # Assert
        assert "DAX.DTB" == str(security)

    def test_future_security_repr(self):
        # Arrange
        security = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
            expiry="201609",
            currency=Currency.from_str("EUR"),
            multiplier=5,
        )

        # Act
        # Assert
        assert "FutureSecurity('DAX.DTB,INDEX,201609,EUR,5')" == repr(security)

    def test_security_equality(self):
        # Arrange
        security1 = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
            expiry="201609",
            currency=Currency.from_str("EUR"),
            multiplier=5,
        )

        security2 = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
            expiry="201610",
            currency=Currency.from_str("EUR"),
            multiplier=5,
        )

        # Act
        # Assert
        assert security1 == security1
        assert security1 != security2

    def test_parse_security_from_str_with_all_fields(self):
        # Arrange
        security = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
            expiry="201609",
            currency=Currency.from_str("EUR"),
            multiplier=5,
        )

        # Act
        result = FutureSecurity.from_str(security.to_serializable_str())

        # Assert
        assert security == result

    def test_parse_security_from_str_with_some_fields(self):
        # Arrange
        security = FutureSecurity(
            symbol=Symbol("DAX"),
            exchange=Exchange("DTB"),
            asset_class=AssetClass.INDEX,
        )

        print(security.to_serializable_str())

        # Act
        result = FutureSecurity.from_str(security.to_serializable_str())

        # Assert
        assert security == result
