# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
