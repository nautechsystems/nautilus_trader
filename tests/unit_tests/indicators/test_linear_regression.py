from nautilus_trader.indicators.linear_regression import LinearRegression
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestLinearRegression:
    def setup(self):
        self.period = 4
        self.linear_regression = LinearRegression(period=self.period)

    def test_init(self):
        assert not self.linear_regression.initialized
        assert not self.linear_regression.has_inputs
        assert self.linear_regression.period == self.period
        assert self.linear_regression.value == 0

    def test_name_returns_expected_string(self):
        assert self.linear_regression.name == "LinearRegression"

    def test_handle_bar_updates_indicator(self):
        for _ in range(self.period):
            self.linear_regression.handle_bar(TestStubs.bar_5decimal())

        assert self.linear_regression.has_inputs
        assert self.linear_regression.value == 1.500045

    def test_value_with_one_input(self):
        self.linear_regression.update_raw(1.00000)
        assert self.linear_regression.value == 0.0

    def test_value_with_ten_inputs(self):
        self.linear_regression.update_raw(1.00000)
        self.linear_regression.update_raw(2.00000)
        self.linear_regression.update_raw(3.00000)
        self.linear_regression.update_raw(4.00000)
        self.linear_regression.update_raw(5.00000)
        self.linear_regression.update_raw(6.00000)
        self.linear_regression.update_raw(7.00000)
        self.linear_regression.update_raw(8.00000)
        self.linear_regression.update_raw(9.00000)
        self.linear_regression.update_raw(10.00000)

        assert self.linear_regression.value == 14.0

    def test_reset(self):
        self.linear_regression.update_raw(1.00000)

        self.linear_regression.reset()

        assert not self.linear_regression.initialized
        assert not self.linear_regression.has_inputs
        assert self.linear_regression.period == self.period
        assert self.linear_regression.value == 0
