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

import random

from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import init_logging
from nautilus_trader.common.component import is_logging_initialized
from nautilus_trader.common.enums import LogLevel


def test_logging(benchmark) -> None:
    random.seed(45362718)
    _guard = None
    if not is_logging_initialized:
        _guard = init_logging(level_stdout=LogLevel.ERROR, bypass=True)

    logger = Logger(name="TEST_LOGGER")

    # messages of varying lengths
    messages = [
        "Initializing positronic matrix",
        "Activating quantum singularity drive",
        "Calibrating transdimensional phase array",
        "Engaging hyperion particle accelerator",
        "Deploying ionized plasma thrusters",
        "Charging graviton emitter array",
        "Initiating tachyon sensor sweep",
        "Activating neural interface protocol",
        "Initializing fusion reactor core",
        "Engaging gravimetric distortion field",
        "Deploying positron matrix containment",
        "Initiating quantum entanglement protocol",
        "Calibrating ion thruster array",
        "Activating plasma conduit system",
        "Charging phase inducer matrix",
        "Engaging gravimetric warp drive",
        "Deploying graviton beam array",
        "Initializing graviton polarity array",
        "Activating tachyon pulse generator",
        "Initiating positron containment field",
        "Initializing multi-phase quantum singularity containment field",
        "Deploying ionized plasma thrusters for interstellar travel",
        "Calibrating neural interface for optimal performance",
        "Engaging gravimetric warp drive for faster-than-light travel",
        "Activating tachyon pulse generator for temporal manipulation",
        "Activating shields",
        "Charging plasma cannon",
        "Deploying tractor beam",
        "Initializing warp drive",
        "Engaging hyperdrive",
    ]

    def run():
        for i in range(100_000):
            message = random.choice(messages)
            # unique log messages to prevent caching during string conversion
            logger.info(f"{i}: {message}")

    benchmark(run)
