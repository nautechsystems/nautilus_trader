from typing import Optional

import click
import fsspec

from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode


@click.command()
@click.option("--raw", help="A raw string config")
@click.option("--fsspec-url", help="A fsspec url to read config from")
@click.option("--start", default=True, help="Start the live node")
def main(
    raw: Optional[str] = None,
    fsspec_url: Optional[str] = None,
    start: bool = True,
):
    assert raw is not None or fsspec_url is not None, "Must pass one of `raw` or `fsspec_url`"
    if fsspec_url and raw is None:
        with fsspec.open(fsspec_url, "rb") as f:
            raw = f.read().decode()
    config = TradingNodeConfig.parse_raw(raw)
    node = TradingNode(config=config)
    node.build()
    if start:
        node.start()


if __name__ == "__main__":
    main()
