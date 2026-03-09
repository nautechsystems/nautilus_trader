import click
import fsspec
import msgspec

from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode


@click.command()
@click.option("--raw", help="A raw string config")
@click.option("--fsspec-url", help="A fsspec url to read config from")
@click.option("--start", default=True, help="Start the live node")
def main(
    raw: str | None = None,
    fsspec_url: str | None = None,
    start: bool = True,
) -> None:
    assert raw is not None or fsspec_url is not None, "Must pass one of `raw` or `fsspec_url`"
    if fsspec_url and raw is None:
        with fsspec.open(fsspec_url, "rb") as f:
            raw = f.read().decode()
    assert raw is not None  # Type checking
    config: TradingNodeConfig = msgspec.json.decode(raw, type=TradingNodeConfig)
    node = TradingNode(config=config)
    node.build()
    if start:
        try:
            node.run()
        finally:
            node.dispose()


if __name__ == "__main__":
    main()
