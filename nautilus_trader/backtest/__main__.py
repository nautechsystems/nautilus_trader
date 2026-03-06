import click
import fsspec
import msgspec

from nautilus_trader.backtest.node import BacktestNode
from nautilus_trader.config import BacktestRunConfig
from nautilus_trader.config import msgspec_decoding_hook


@click.command()
@click.option("--raw", help="A raw string configs list")
@click.option("--fsspec-url", help="A fsspec url to read a list of configs from")
def main(
    raw: str | None = None,
    fsspec_url: str | None = None,
) -> None:
    if raw is None and fsspec_url is None:
        raise ValueError("Must pass one of `raw` or `fsspec_url`")

    if fsspec_url and raw is None:
        with fsspec.open(fsspec_url, "rb") as f:
            data = f.read().decode()
    else:
        assert raw is not None  # Type checking
        data = raw.encode()

    configs = msgspec.json.decode(
        data,
        type=list[BacktestRunConfig],
        dec_hook=msgspec_decoding_hook,
    )
    node = BacktestNode(configs=configs)
    node.run()


if __name__ == "__main__":
    main()
