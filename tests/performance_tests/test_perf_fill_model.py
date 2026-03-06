from nautilus_trader.backtest.models import FillModel


_FILL_MODEL = FillModel(
    prob_fill_on_limit=0.5,
    random_seed=42,
)


def test_is_limit_filled(benchmark):
    benchmark(_FILL_MODEL.is_limit_filled)
