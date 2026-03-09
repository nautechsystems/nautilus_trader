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
