from nautilus_trader.indicators.helpers import cross_over, cross_up, cross_down


class TestCrossOverHelpers:
    def setup(self):
        self.input1 = [1, 2]
        self.input2 = [2, 1]

    def test_cross_over_direction_up(self):
        assert cross_over(direction="UP", values1=self.input1, values2=self.input2)
        assert not cross_over(direction="UP", values1=self.input2, values2=self.input1)

    def test_cross_over_direction_down(self):
        assert cross_over(direction="DOWN", values1=self.input2, values2=self.input1)
        assert not cross_over(direction="DOWN", values1=self.input1, values2=self.input2)

    def test_cross_up(self):
        assert cross_up(values1=self.input1, values2=self.input2)
        assert not cross_up(values1=self.input2, values2=self.input1)

    def test_cross_down(self):
        assert cross_down(values1=self.input2, values2=self.input1)
        assert not cross_down(values1=self.input1, values2=self.input2)
