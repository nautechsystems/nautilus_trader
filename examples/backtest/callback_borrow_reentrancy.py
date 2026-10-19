# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Reproduce PyO3 borrow errors from callback reentry.

`start` propagates a RuntimeError from on_start back to the caller of actor.start().
Failed startup leaves the actor in Starting, so disposal also logs an invalid state transition.
`signal` logs a RuntimeError from the nested on_signal callback and continues.

Run from the repository root:
    python/.venv/bin/python examples/backtest/callback_borrow_reentrancy.py start

"""

import argparse

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.common import DataActor
from nautilus_trader.common import LogLevel
from nautilus_trader.common import Signal
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.config import LoggerConfig


class SubscribingActor(DataActor):
    """
    Expose Python actor borrow conflicts during startup or nested publication.
    """

    def on_start(self) -> None:
        """
        Subscribe from the startup callback.
        """
        print("A:on_start: subscribing to signals", flush=True)
        self.subscribe_signal("reentry")
        print("A:on_start: subscription returned", flush=True)

    def on_signal(self, signal: Signal) -> None:
        """
        Publish recursively, then unsubscribe from the nested callback.
        """
        print(f"A:{signal.value}:enter", flush=True)
        if signal.value == "outer":
            self.publish_signal("reentry", "inner", ts_event=2)
        else:
            print("A:inner: calling unsubscribe_signal", flush=True)
            self.unsubscribe_signal("reentry")
            print("A:inner: unsubscribe returned", flush=True)
        print(f"A:{signal.value}:exit", flush=True)


class InitialPublisher(DataActor):
    """
    Start publication after the subscribing actor has started.
    """

    def on_start(self) -> None:
        """
        Publish the outer signal and print when it returns.
        """
        print("PUBLISHER: publish outer", flush=True)
        self.publish_signal("reentry", "outer", ts_event=1)
        print("PUBLISHER: publication returned", flush=True)


def main() -> None:
    """
    Run the selected Python actor borrow conflict scenario.
    """
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("scenario", choices=("start", "signal"), nargs="?", default="start")
    args = parser.parse_args()
    print(f"Scenario: {args.scenario}", flush=True)
    engine = BacktestEngine(
        BacktestEngineConfig(
            logging=LoggerConfig(stdout_level=LogLevel.WARNING),
            run_analysis=False,
        ),
    )
    actor = SubscribingActor()
    engine.add_actor(actor)
    try:
        if args.scenario == "start":
            print("CALLER: actor.start()", flush=True)
            actor.start()
            print("CALLER: actor.start() returned", flush=True)
        else:
            engine.add_actor(InitialPublisher())
            engine.run()
            print("CALLER: engine.run() returned", flush=True)
    finally:
        engine.dispose()


if __name__ == "__main__":
    main()
