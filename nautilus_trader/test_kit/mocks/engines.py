from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.live.execution_engine import LiveExecutionEngine
from nautilus_trader.live.risk_engine import LiveRiskEngine


class MockLiveDataEngine(LiveDataEngine):
    """
    Provides a mock live data engine for testing.
    """

    def __init__(
        self,
        loop,
        msgbus,
        cache,
        clock,
        config=None,
    ):
        super().__init__(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self.commands = []
        self.events = []
        self.responses = []

    def execute(self, command):
        self.commands.append(command)

    def process(self, data):
        self.events.append(data)

    def receive(self, response):
        self.responses.append(response)


class MockLiveExecutionEngine(LiveExecutionEngine):
    """
    Provides a mock live execution engine for testing.
    """

    def __init__(
        self,
        loop,
        msgbus,
        cache,
        clock,
        config=None,
    ):
        super().__init__(
            loop=loop,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self.commands = []
        self.events = []

    def execute(self, command):
        self.commands.append(command)

    def process(self, event):
        self.events.append(event)


class MockLiveRiskEngine(LiveRiskEngine):
    """
    Provides a mock live risk engine for testing.
    """

    def __init__(
        self,
        loop,
        portfolio,
        msgbus,
        cache,
        clock,
        config=None,
    ):
        super().__init__(
            loop=loop,
            portfolio=portfolio,
            msgbus=msgbus,
            cache=cache,
            clock=clock,
            config=config,
        )

        self.commands = []
        self.events = []

    def execute(self, command):
        self.commands.append(command)

    def process(self, event):
        self.events.append(event)
