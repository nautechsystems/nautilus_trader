from nautilus_trader.model.currencies import USDC_POS
from nautilus_trader.model.objects import HIGH_PRECISION
from nautilus_trader.model.objects import Money


def usdce_from_units(units: int) -> Money:
    """
    Return USDC.e money from the given units amount.

    Parameters
    ----------
    units : int
        The amount as an integer of fractional subunits.

    Returns
    -------
    Money

    """
    if HIGH_PRECISION:
        factor = 10_000_000_000
    else:
        factor = 1_000

    return Money.from_raw(int(units * factor), USDC_POS)
