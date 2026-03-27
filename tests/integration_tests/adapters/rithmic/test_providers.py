"""Tests for Rithmic instrument provider."""

from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig, RithmicEnvironment
from nautilus_trader.adapters.rithmic.providers import (
    RITHMIC_VENUE,
    RithmicInstrumentProvider,
    normalize_rithmic_symbol,
    resolve_exchange_hint,
)
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.identifiers import InstrumentId


class TestRithmicInstrumentProvider:
    """Tests for RithmicInstrumentProvider."""

    def test_create_provider(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="test_user",
            password="test_pass",
            system_name="test_system",
        )
        provider = RithmicInstrumentProvider(config)
        assert provider.venue == RITHMIC_VENUE

    def test_get_all_empty(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="test_user",
            password="test_pass",
            system_name="test_system",
        )
        provider = RithmicInstrumentProvider(config)
        instruments = provider.get_all()
        assert instruments == {}

    def test_resolve_exchange_hint_from_filters(self):
        assert resolve_exchange_hint("ESZ4", {"exchange": "CME"}) == "CME"

    def test_resolve_exchange_hint_from_symbol_suffix(self):
        assert resolve_exchange_hint("ESZ4.CME") == "CME"
        assert resolve_exchange_hint("ESZ4:NYMEX") == "NYMEX"

    def test_normalize_rithmic_symbol_strips_exchange_suffix(self):
        assert normalize_rithmic_symbol("ESZ4.CME") == "ESZ4"
        assert normalize_rithmic_symbol("ESZ4:CBOT") == "ESZ4"
        assert normalize_rithmic_symbol("ESZ4") == "ESZ4"

    def test_find_uses_normalized_instrument_id(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="test_user",
            password="test_pass",
            system_name="test_system",
        )
        provider = RithmicInstrumentProvider(config)
        normalized_id = InstrumentId.from_str("ESZ4.RITHMIC")
        provider._instruments[normalized_id] = object()

        instrument = provider.find(InstrumentId.from_str("ESZ4.CME.RITHMIC"))
        assert instrument is provider._instruments[normalized_id]

    def test_provider_uses_nested_instrument_provider_config(self):
        config = RithmicDataClientConfig(
            environment=RithmicEnvironment.DEMO,
            username="test_user",
            password="test_pass",
            system_name="test_system",
            instrument_provider=InstrumentProviderConfig(
                load_all=True,
                filters={"exchange": "CME"},
            ),
        )

        provider = RithmicInstrumentProvider(config)

        assert provider._load_all_on_start is True
        assert provider._filters == {"exchange": "CME"}

    # Note: Actual async tests would require pytest-asyncio
    # and mocking the Rithmic connection
    #
    # @pytest.mark.asyncio
    # async def test_load_all_async(self):
    #     config = RithmicDataClientConfig.from_env()
    #     provider = RithmicInstrumentProvider(config)
    #     await provider.load_all_async()
    #     assert len(provider.get_all()) > 0
