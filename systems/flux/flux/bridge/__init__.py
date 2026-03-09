"""
Flux bridge stream consumer and modular topic handlers.
"""

from flux.bridge.stream_consumer import FluxBridgeStreamConsumer
from flux.bridge.stream_consumer import build_parser
from flux.bridge.stream_consumer import main


__all__ = ["FluxBridgeStreamConsumer", "build_parser", "main"]
