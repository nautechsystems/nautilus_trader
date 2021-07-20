from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Venue


TEMPLATE_VENUE = Venue("TEMPLATE")


class TemplateInstrumentProvider(InstrumentProvider):
    async def load_all_async(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def load_all(self):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")

    def load(self, instrument_id: InstrumentId, details: dict):
        """Abstract method (implement in subclass)."""
        raise NotImplementedError("method must be implemented in the subclass")
