from __future__ import annotations

from decimal import Decimal

from nautilus_trader.flux.common.quantity_units import exposure_from_venue_qty
from nautilus_trader.flux.common.quantity_units import venue_qty_from_base_qty
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.providers import TestInstrumentProvider


def _okx_linear_perpetual(
    *,
    settlement_currency=USDT,
    info: dict[str, object] | None = None,
) -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("ETH-USDT-SWAP"),
            venue=Venue("OKX"),
        ),
        raw_symbol=Symbol("ETH-USDT-SWAP"),
        base_currency=ETH,
        quote_currency=USDT,
        settlement_currency=settlement_currency,
        is_inverse=False,
        price_precision=4,
        size_precision=0,
        price_increment=Price.from_str("0.0001"),
        size_increment=Quantity.from_str("1"),
        multiplier=Quantity.from_str("10"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info=info,
    )


def _okx_inverse_perpetual(
    *,
    info: dict[str, object] | None = None,
) -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("BTC-USD-SWAP"),
            venue=Venue("OKX"),
        ),
        raw_symbol=Symbol("BTC-USD-SWAP"),
        base_currency=BTC,
        quote_currency=USD,
        settlement_currency=BTC,
        is_inverse=True,
        price_precision=1,
        size_precision=0,
        price_increment=Price.from_str("0.1"),
        size_increment=Quantity.from_str("1"),
        multiplier=Quantity.from_str("100"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info=info,
    )


def _hyperliquid_identity_perpetual() -> CryptoPerpetual:
    return CryptoPerpetual(
        instrument_id=InstrumentId(
            symbol=Symbol("xyz:AAPL-USD-PERP"),
            venue=Venue("HYPERLIQUID"),
        ),
        raw_symbol=Symbol("xyz:AAPL"),
        base_currency=Currency.from_str("xyz:AAPL"),
        quote_currency=USD,
        settlement_currency=USDC,
        is_inverse=False,
        price_precision=3,
        size_precision=3,
        price_increment=Price.from_str("0.001"),
        size_increment=Quantity.from_str("0.001"),
        multiplier=Quantity.from_str("1"),
        lot_size=Quantity.from_str("1"),
        ts_event=0,
        ts_init=0,
        info={"base_exposure_mode": "identity"},
    )


def test_exposure_from_venue_qty_identity_for_spot_instrument() -> None:
    instrument = TestInstrumentProvider.ethusdt_binance()

    exposure = exposure_from_venue_qty(instrument, Decimal("1.25"))

    assert exposure.venue_qty == Decimal("1.25")
    assert exposure.base_qty == Decimal("1.25")
    assert exposure.qty_conversion_status == "identity"
    assert exposure.qty_conversion_source == "generic:multiplier=1"


def test_exposure_from_venue_qty_uses_exact_multiplier_metadata() -> None:
    instrument = _okx_linear_perpetual(
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "ETH",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
    )

    exposure = exposure_from_venue_qty(instrument, Decimal("343"))

    assert exposure.venue_qty == Decimal("343")
    assert exposure.base_qty == Decimal("3430")
    assert exposure.qty_conversion_status == "exact_multiplier"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=exact_multiplier"


def test_exposure_from_venue_qty_preserves_short_position_sign() -> None:
    instrument = _okx_linear_perpetual(
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "ETH",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
    )

    exposure = exposure_from_venue_qty(instrument, Decimal("-343"))

    assert exposure.venue_qty == Decimal("-343")
    assert exposure.base_qty == Decimal("-3430")
    assert exposure.qty_conversion_status == "exact_multiplier"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=exact_multiplier"


def test_exposure_from_venue_qty_requires_price_for_price_based_conversion() -> None:
    instrument = _okx_inverse_perpetual(
        info={
            "okx_ct_val": "100",
            "okx_ct_val_ccy": "USD",
            "okx_ct_type": "inverse",
            "okx_lot_sz": "1",
            "base_exposure_mode": "price_based",
        },
    )

    exposure = exposure_from_venue_qty(instrument, Decimal("100"))

    assert exposure.venue_qty == Decimal("100")
    assert exposure.base_qty is None
    assert exposure.qty_conversion_status == "missing_price"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=price_based requires last_px"


def test_exposure_from_venue_qty_degrades_cleanly_for_unsupported_metadata() -> None:
    instrument = _okx_linear_perpetual(
        settlement_currency=USDC,
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "ETH",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "unsupported",
        },
    )

    exposure = exposure_from_venue_qty(instrument, Decimal("343"))

    assert exposure.venue_qty == Decimal("343")
    assert exposure.base_qty is None
    assert exposure.qty_conversion_status == "unsupported"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=unsupported"


def test_exposure_from_venue_qty_flags_incomplete_okx_metadata() -> None:
    instrument = _okx_linear_perpetual(
        info={
            "okx_ct_val": "",
            "okx_ct_val_ccy": "ETH",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "unsupported",
        },
    )

    exposure = exposure_from_venue_qty(instrument, Decimal("343"))

    assert exposure.venue_qty == Decimal("343")
    assert exposure.base_qty is None
    assert exposure.qty_conversion_status == "missing_metadata"
    assert exposure.qty_conversion_source == "instrument.info:incomplete_okx_quantity_unit_metadata"


def test_exposure_from_venue_qty_honors_identity_metadata_without_core_quanto_math() -> None:
    instrument = _hyperliquid_identity_perpetual()

    exposure = exposure_from_venue_qty(instrument, Decimal("1"))

    assert exposure.venue_qty == Decimal("1")
    assert exposure.base_qty == Decimal("1")
    assert exposure.qty_conversion_status == "identity"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=identity"


def test_venue_qty_from_base_qty_round_trips_exact_multiplier_conversion() -> None:
    instrument = _okx_linear_perpetual(
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "ETH",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
    )

    exposure = venue_qty_from_base_qty(instrument, Decimal("3430"))

    assert exposure.venue_qty == Decimal("343")
    assert exposure.base_qty == Decimal("3430")
    assert exposure.qty_conversion_status == "exact_multiplier"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=exact_multiplier"


def test_venue_qty_from_base_qty_preserves_short_base_exposure_sign() -> None:
    instrument = _okx_linear_perpetual(
        info={
            "okx_ct_val": "10",
            "okx_ct_val_ccy": "ETH",
            "okx_ct_type": "linear",
            "okx_lot_sz": "1",
            "base_exposure_mode": "exact_multiplier",
        },
    )

    exposure = venue_qty_from_base_qty(instrument, Decimal("-3430"))

    assert exposure.venue_qty == Decimal("-343")
    assert exposure.base_qty == Decimal("-3430")
    assert exposure.qty_conversion_status == "exact_multiplier"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=exact_multiplier"


def test_venue_qty_from_base_qty_honors_identity_metadata_without_quanto_roundtrip_failure() -> None:
    instrument = _hyperliquid_identity_perpetual()

    exposure = venue_qty_from_base_qty(instrument, Decimal("1"))

    assert exposure.venue_qty == Decimal("1")
    assert exposure.base_qty == Decimal("1")
    assert exposure.qty_conversion_status == "identity"
    assert exposure.qty_conversion_source == "instrument.info:base_exposure_mode=identity"
