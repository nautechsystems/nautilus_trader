# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

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
