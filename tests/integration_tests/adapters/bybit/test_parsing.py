import pytest

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser, BybitOrderSide, BybitOrderStatus
from nautilus_trader.model.data import BarType
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus


ETHUSDT_BYBIT = TestInstrumentProvider.ethusdt_binance()

class TestBybitParsing:
    def setup(self):
        self._enum_parser = BybitEnumParser()
        self.instrument: str = "ETHUSDT.BINANCE"

    @pytest.mark.parametrize(
        ("bar_type", "bybit_kline_interval"),
        [
            [f"ETHUSDT.BYBIT-1-MINUTE-LAST-EXTERNAL",'1'],
            [f"ETHUSDT.BYBIT-3-MINUTE-LAST-EXTERNAL",'3'],
            [f"ETHUSDT.BYBIT-5-MINUTE-LAST-EXTERNAL",'5'],
            [f"ETHUSDT.BYBIT-15-MINUTE-LAST-EXTERNAL",'15'],
            [f"ETHUSDT.BYBIT-30-MINUTE-LAST-EXTERNAL",'30'],
            [f"ETHUSDT.BYBIT-1-HOUR-LAST-EXTERNAL",'60'],
            [f"ETHUSDT.BYBIT-2-HOUR-LAST-EXTERNAL",'120'],
            [f"ETHUSDT.BYBIT-4-HOUR-LAST-EXTERNAL", '240'],
            [f"ETHUSDT.BYBIT-6-HOUR-LAST-EXTERNAL", '360'],
            [f"ETHUSDT.BYBIT-12-HOUR-LAST-EXTERNAL", '720'],
            [f"ETHUSDT.BYBIT-1-DAY-LAST-EXTERNAL", 'D'],
            [f"ETHUSDT.BYBIT-1-WEEK-LAST-EXTERNAL", 'W'],
            [f"ETHUSDT.BYBIT-1-MONTH-LAST-EXTERNAL", 'M'],
        ])
    def test_parse_bybit_kline_correct(self,bar_type,bybit_kline_interval):
        bar_type = BarType.from_str(bar_type)
        result = self._enum_parser.parse_bybit_kline(bar_type)
        assert result.value == bybit_kline_interval


    def test_parse_bybit_kline_incorrect(self):
        # MINUTE
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(BarType.from_str("ETHUSDT.BYBIT-2-MINUTE-LAST-EXTERNAL"))
        # HOUR
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(BarType.from_str("ETHUSDT.BYBIT-3-HOUR-LAST-EXTERNAL"))
        # DAY
        with pytest.raises(ValueError):
            result = self._enum_parser.parse_bybit_kline(BarType.from_str("ETHUSDT.BYBIT-3-DAY-LAST-EXTERNAL"))
            print(result)
        # WEEK
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(BarType.from_str("ETHUSDT.BYBIT-2-WEEK-LAST-EXTERNAL"))
        # MONTH
        with pytest.raises(ValueError):
            self._enum_parser.parse_bybit_kline(BarType.from_str("ETHUSDT.BYBIT-4-MONTH-LAST-EXTERNAL"))

    @pytest.mark.parametrize(
        ("bybit_order_side","order_side"),
        [
            [BybitOrderSide.BUY, OrderSide.BUY],
            [BybitOrderSide.SELL, OrderSide.SELL],
        ]
    )
    def test_parse_bybit_order_side(self,bybit_order_side,order_side):
        result = self._enum_parser.parse_bybit_order_side(bybit_order_side)
        assert result == order_side


    @pytest.mark.parametrize(
        ("order_side","bybit_order_side"),
        [
            [OrderSide.BUY, BybitOrderSide.BUY],
            [OrderSide.SELL, BybitOrderSide.SELL],
        ]
    )
    def test_parse_nautilus_order_side(self,order_side,bybit_order_side):
        result = self._enum_parser.parse_nautilus_order_side(order_side)
        assert result == bybit_order_side

    @pytest.mark.parametrize(
        ("bybit_order_status","order_status"),
        [
            [BybitOrderStatus.CREATED, OrderStatus.SUBMITTED],
            [BybitOrderStatus.NEW,OrderStatus.ACCEPTED],
            [BybitOrderStatus.FILLED,OrderStatus.FILLED],
            [BybitOrderStatus.CANCELLED,OrderStatus.CANCELLED],
        ]
    )
    def test_parse_bybit_order_status(self,bybit_order_status,order_status):
        result = self._enum_parser.parse_bybit_order_status(bybit_order_status)
        assert result == order_status

    @pytest.mark.parametrize(
        ("order_status","bybit_order_status"),
        [
            [OrderStatus.SUBMITTED, BybitOrderStatus.CREATED],
            [OrderStatus.ACCEPTED,BybitOrderStatus.NEW],
            [OrderStatus.FILLED,BybitOrderStatus.FILLED],
            [OrderStatus.CANCELLED,BybitOrderStatus.CANCELLED],
        ]
    )
    def test_parse_nautilus_order_status(self,order_status,bybit_order_status):
        result = self._enum_parser.parse_nautilus_order_status(order_status)
        assert result == bybit_order_status

