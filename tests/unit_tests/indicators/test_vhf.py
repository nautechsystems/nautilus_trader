from nautilus_trader.indicators import VerticalHorizontalFilter
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


class TestVerticalHorizontalFilter:
    def setup(self):
        # Fixture Setup
        self.period = 10
        self.vhf = VerticalHorizontalFilter(period=self.period)

    def test_init(self):
        assert not self.vhf.initialized
        assert not self.vhf.has_inputs
        assert self.vhf.period == self.period
        assert self.vhf.value == 0

    def test_name_returns_expected_string(self):
        assert self.vhf.name == "VerticalHorizontalFilter"

    def test_handle_bar_updates_indicator(self):
        for _ in range(self.period):
            self.vhf.handle_bar(TestDataStubs.bar_5decimal())

        assert self.vhf.has_inputs
        assert self.vhf.value == 0

    def test_value_with_one_input(self):
        self.vhf.update_raw(56.87)

        assert self.vhf.value == 0

    def test_value_with_twenty_inputs(self):
        self.vhf.update_raw(56.87)
        self.vhf.update_raw(56.96)
        self.vhf.update_raw(57.17)
        self.vhf.update_raw(57.54)
        self.vhf.update_raw(57.88)
        self.vhf.update_raw(57.85)
        self.vhf.update_raw(57.86)
        self.vhf.update_raw(57.97)
        self.vhf.update_raw(58.07)
        self.vhf.update_raw(58.04)
        self.vhf.update_raw(57.96)
        self.vhf.update_raw(57.98)
        self.vhf.update_raw(58.05)
        self.vhf.update_raw(57.94)
        self.vhf.update_raw(57.99)
        self.vhf.update_raw(58.11)
        self.vhf.update_raw(58.22)
        self.vhf.update_raw(58.19)
        self.vhf.update_raw(58.04)
        self.vhf.update_raw(58.02)

        assert self.vhf.value == 0.36842105263158487

    def test_reset(self):
        self.vhf.update_raw(56.87)

        self.vhf.reset()

        assert not self.vhf.initialized
        assert not self.vhf.has_inputs
        assert self.vhf.period == self.period
        assert self.vhf.value == 0
