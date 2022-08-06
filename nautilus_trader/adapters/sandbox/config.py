from nautilus_trader.config import LiveExecClientConfig


class SandboxExecutionClientConfig(LiveExecClientConfig):
    """
    Configuration for ``SandboxExecClient`` instances.

    Parameters
    ----------
    venue : str
        The venue to generate a sandbox execution client for
    currency: str
        The currency for this venue
    balance : int
        The starting balance for this venue
    """

    venue: str
    currency: str
    balance: int
