import pytest

from nautilus_trader.adapters._template.providers import TemplateInstrumentProvider


pytestmark = pytest.mark.skip(reason="template")


@pytest.fixture
def instrument_provider():
    return TemplateInstrumentProvider()


def test_load_all_async(instrument_provider):
    pass


def test_load_all(instrument_provider):
    pass


def test_load(instrument_provider):
    pass
