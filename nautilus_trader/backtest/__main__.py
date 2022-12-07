from typing import Optional

import click
import fsspec
import msgspec

from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestRunConfig


@click.command()
@click.option("--raw", help="A raw string configs list")
@click.option("--fsspec-url", help="A fsspec url to read a list of configs from")
def main(
    raw: Optional[str] = None,
    fsspec_url: Optional[str] = None,
):
    assert raw is not None or fsspec_url is not None, "Must pass one of `raw` or `fsspec_url`"
    if fsspec_url and raw is None:
        with fsspec.open(fsspec_url, "rb") as f:
            data = f.read().decode()
    else:
        data = raw.encode()
    configs = msgspec.json.decode(data, type=list[BacktestRunConfig])
    node = BacktestNode(configs=configs)
    node.run()


if __name__ == "__main__":
    main()
