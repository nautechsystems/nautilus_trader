#!/usr/bin/env python3
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

"""
Performance tests for high-frequency data processing in Delta Exchange adapter.

This module tests the adapter's performance under high-frequency data scenarios,
measuring throughput, latency, memory usage, and resource efficiency.
"""

import asyncio
import gc
import json
import psutil
import time
from collections import defaultdict
from statistics import mean, median, stdev
from unittest.mock import AsyncMock, MagicMock

import pytest

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE_VENUE
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.model.data import QuoteTick, TradeTick
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.test_kit.mocks import MockMessageBus


@pytest.mark.performance
class TestHighFrequencyProcessing:
    """Test high-frequency data processing performance."""

    def setup_method(self):
        """Set up test fixtures."""
        self.loop = asyncio.get_event_loop()
        self.clock = LiveClock()
        self.msgbus = MockMessageBus()
        self.cache = Cache()
        
        # Performance tracking
        self.processing_times = []
        self.memory_usage = []
        self.message_counts = defaultdict(int)
        self.latencies = []
        
        # Test configuration
        self.config = DeltaExchangeDataClientConfig(
            api_key="test_key",
            api_secret="test_secret",
            testnet=True,
            enable_private_channels=False,
            product_types=["perpetual_futures"],
            request_timeout_secs=30.0,
            ws_timeout_secs=10.0,
        )
        
        # Test instruments
        self.test_instruments = [
            InstrumentId(Symbol("BTCUSDT"), DELTA_EXCHANGE_VENUE),
            InstrumentId(Symbol("ETHUSDT"), DELTA_EXCHANGE_VENUE),
            InstrumentId(Symbol("SOLUSDT"), DELTA_EXCHANGE_VENUE),
            InstrumentId(Symbol("ADAUSDT"), DELTA_EXCHANGE_VENUE),
            InstrumentId(Symbol("DOTUSDT"), DELTA_EXCHANGE_VENUE),
        ]

    def _create_mock_ticker_message(self, symbol: str, timestamp: int) -> dict:
        """Create a mock ticker message."""
        return {
            "type": "v2_ticker",
            "product_id": 139,
            "symbol": symbol,
            "timestamp": timestamp,
            "best_bid": "50000.0",
            "best_ask": "50050.0",
            "last_price": "50025.0",
            "volume": "1234.567",
            "open_interest": "9876.543",
            "mark_price": "50025.0",
            "funding_rate": "0.0001",
            "next_funding_time": timestamp + 28800000000
        }

    def _create_mock_trade_message(self, symbol: str, timestamp: int) -> dict:
        """Create a mock trade message."""
        return {
            "type": "all_trades",
            "product_id": 139,
            "symbol": symbol,
            "timestamp": timestamp,
            "price": "50000.0",
            "size": "0.5",
            "side": "buy",
            "trade_id": f"trade_{timestamp}"
        }

    def _create_mock_orderbook_message(self, symbol: str, timestamp: int) -> dict:
        """Create a mock order book message."""
        return {
            "type": "l2_orderbook",
            "product_id": 139,
            "symbol": symbol,
            "timestamp": timestamp,
            "buy": [
                {"price": "49950.0", "size": "1.5"},
                {"price": "49900.0", "size": "2.0"},
                {"price": "49850.0", "size": "1.0"}
            ],
            "sell": [
                {"price": "50050.0", "size": "1.2"},
                {"price": "50100.0", "size": "1.8"},
                {"price": "50150.0", "size": "0.9"}
            ]
        }

    def _setup_performance_monitoring(self, data_client):
        """Set up performance monitoring handlers."""
        def on_quote_tick(quote: QuoteTick):
            self.message_counts["quotes"] += 1
            # Calculate latency (mock)
            current_time = time.time_ns()
            latency = (current_time - quote.ts_event) / 1_000_000  # Convert to milliseconds
            self.latencies.append(latency)
        
        def on_trade_tick(trade: TradeTick):
            self.message_counts["trades"] += 1
            # Calculate latency (mock)
            current_time = time.time_ns()
            latency = (current_time - trade.ts_event) / 1_000_000  # Convert to milliseconds
            self.latencies.append(latency)
        
        # Register handlers
        self.msgbus.register_handler(QuoteTick, on_quote_tick)
        self.msgbus.register_handler(TradeTick, on_trade_tick)

    @pytest.mark.asyncio
    async def test_high_frequency_ticker_processing(self):
        """Test processing of high-frequency ticker messages."""
        # Create mock provider and client
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_performance_monitoring(data_client)
        
        # Generate high-frequency ticker messages
        message_count = 10000
        start_time = time.time()
        base_timestamp = int(time.time() * 1_000_000)
        
        # Record initial memory usage
        process = psutil.Process()
        initial_memory = process.memory_info().rss / 1024 / 1024  # MB
        
        # Process messages
        for i in range(message_count):
            timestamp = base_timestamp + i * 1000  # 1ms intervals
            for instrument in self.test_instruments:
                message = self._create_mock_ticker_message(instrument.symbol.value, timestamp)
                
                # Simulate message processing
                processing_start = time.perf_counter()
                await data_client._handle_ws_message(json.dumps(message))
                processing_end = time.perf_counter()
                
                self.processing_times.append((processing_end - processing_start) * 1000)  # ms
        
        end_time = time.time()
        final_memory = process.memory_info().rss / 1024 / 1024  # MB
        
        # Calculate performance metrics
        total_time = end_time - start_time
        total_messages = message_count * len(self.test_instruments)
        throughput = total_messages / total_time
        
        avg_processing_time = mean(self.processing_times)
        median_processing_time = median(self.processing_times)
        p95_processing_time = sorted(self.processing_times)[int(0.95 * len(self.processing_times))]
        
        memory_usage = final_memory - initial_memory
        
        # Performance assertions
        assert throughput > 1000, f"Throughput too low: {throughput:.2f} msg/s"
        assert avg_processing_time < 1.0, f"Average processing time too high: {avg_processing_time:.3f}ms"
        assert p95_processing_time < 5.0, f"P95 processing time too high: {p95_processing_time:.3f}ms"
        assert memory_usage < 100, f"Memory usage too high: {memory_usage:.2f}MB"
        
        print(f"High-frequency ticker processing results:")
        print(f"  Throughput: {throughput:.2f} messages/second")
        print(f"  Average processing time: {avg_processing_time:.3f}ms")
        print(f"  Median processing time: {median_processing_time:.3f}ms")
        print(f"  P95 processing time: {p95_processing_time:.3f}ms")
        print(f"  Memory usage: {memory_usage:.2f}MB")

    @pytest.mark.asyncio
    async def test_concurrent_instrument_processing(self):
        """Test concurrent processing of multiple instruments."""
        # Create mock provider and client
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_performance_monitoring(data_client)
        
        # Create concurrent message processing tasks
        async def process_instrument_messages(instrument: InstrumentId, message_count: int):
            """Process messages for a single instrument."""
            base_timestamp = int(time.time() * 1_000_000)
            
            for i in range(message_count):
                timestamp = base_timestamp + i * 1000
                
                # Mix of message types
                if i % 3 == 0:
                    message = self._create_mock_ticker_message(instrument.symbol.value, timestamp)
                elif i % 3 == 1:
                    message = self._create_mock_trade_message(instrument.symbol.value, timestamp)
                else:
                    message = self._create_mock_orderbook_message(instrument.symbol.value, timestamp)
                
                await data_client._handle_ws_message(json.dumps(message))
                
                # Small delay to simulate realistic timing
                await asyncio.sleep(0.001)  # 1ms
        
        # Start concurrent processing
        start_time = time.time()
        process = psutil.Process()
        initial_memory = process.memory_info().rss / 1024 / 1024  # MB
        
        tasks = [
            process_instrument_messages(instrument, 1000)
            for instrument in self.test_instruments
        ]
        
        await asyncio.gather(*tasks)
        
        end_time = time.time()
        final_memory = process.memory_info().rss / 1024 / 1024  # MB
        
        # Calculate metrics
        total_time = end_time - start_time
        total_messages = 1000 * len(self.test_instruments)
        throughput = total_messages / total_time
        memory_usage = final_memory - initial_memory
        
        # Performance assertions
        assert throughput > 500, f"Concurrent throughput too low: {throughput:.2f} msg/s"
        assert memory_usage < 200, f"Concurrent memory usage too high: {memory_usage:.2f}MB"
        
        print(f"Concurrent instrument processing results:")
        print(f"  Instruments: {len(self.test_instruments)}")
        print(f"  Total messages: {total_messages}")
        print(f"  Throughput: {throughput:.2f} messages/second")
        print(f"  Memory usage: {memory_usage:.2f}MB")

    @pytest.mark.asyncio
    async def test_memory_efficiency_under_load(self):
        """Test memory efficiency under sustained load."""
        # Create mock provider and client
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_performance_monitoring(data_client)
        
        # Monitor memory usage over time
        process = psutil.Process()
        memory_samples = []
        
        # Run sustained load test
        for batch in range(10):  # 10 batches
            batch_start_memory = process.memory_info().rss / 1024 / 1024  # MB
            
            # Process a batch of messages
            base_timestamp = int(time.time() * 1_000_000) + batch * 1000000
            
            for i in range(1000):  # 1000 messages per batch
                timestamp = base_timestamp + i * 1000
                instrument = self.test_instruments[i % len(self.test_instruments)]
                
                message = self._create_mock_ticker_message(instrument.symbol.value, timestamp)
                await data_client._handle_ws_message(json.dumps(message))
            
            batch_end_memory = process.memory_info().rss / 1024 / 1024  # MB
            memory_samples.append(batch_end_memory)
            
            # Force garbage collection
            gc.collect()
            
            # Small delay between batches
            await asyncio.sleep(0.1)
        
        # Analyze memory usage
        initial_memory = memory_samples[0]
        final_memory = memory_samples[-1]
        max_memory = max(memory_samples)
        memory_growth = final_memory - initial_memory
        
        # Memory efficiency assertions
        assert memory_growth < 50, f"Memory growth too high: {memory_growth:.2f}MB"
        assert max_memory - initial_memory < 100, f"Peak memory usage too high: {max_memory - initial_memory:.2f}MB"
        
        print(f"Memory efficiency results:")
        print(f"  Initial memory: {initial_memory:.2f}MB")
        print(f"  Final memory: {final_memory:.2f}MB")
        print(f"  Peak memory: {max_memory:.2f}MB")
        print(f"  Memory growth: {memory_growth:.2f}MB")

    @pytest.mark.asyncio
    async def test_latency_under_load(self):
        """Test message processing latency under load."""
        # Create mock provider and client
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        # Track latencies
        latencies = []
        
        def on_quote_tick(quote: QuoteTick):
            # Calculate end-to-end latency
            current_time = time.time_ns()
            latency = (current_time - quote.ts_event) / 1_000_000  # Convert to milliseconds
            latencies.append(latency)
        
        self.msgbus.register_handler(QuoteTick, on_quote_tick)
        
        # Generate messages with realistic timing
        message_count = 5000
        base_timestamp = time.time_ns()
        
        for i in range(message_count):
            # Simulate realistic message timing (some clustering)
            if i % 100 == 0:
                await asyncio.sleep(0.01)  # 10ms gap every 100 messages
            
            timestamp = base_timestamp + i * 1_000_000  # 1ms intervals
            instrument = self.test_instruments[i % len(self.test_instruments)]
            
            message = self._create_mock_ticker_message(instrument.symbol.value, timestamp)
            await data_client._handle_ws_message(json.dumps(message))
        
        # Analyze latencies
        if latencies:
            avg_latency = mean(latencies)
            median_latency = median(latencies)
            p95_latency = sorted(latencies)[int(0.95 * len(latencies))]
            p99_latency = sorted(latencies)[int(0.99 * len(latencies))]
            
            # Latency assertions
            assert avg_latency < 10.0, f"Average latency too high: {avg_latency:.3f}ms"
            assert p95_latency < 50.0, f"P95 latency too high: {p95_latency:.3f}ms"
            assert p99_latency < 100.0, f"P99 latency too high: {p99_latency:.3f}ms"
            
            print(f"Latency under load results:")
            print(f"  Messages processed: {len(latencies)}")
            print(f"  Average latency: {avg_latency:.3f}ms")
            print(f"  Median latency: {median_latency:.3f}ms")
            print(f"  P95 latency: {p95_latency:.3f}ms")
            print(f"  P99 latency: {p99_latency:.3f}ms")

    @pytest.mark.asyncio
    async def test_cpu_efficiency(self):
        """Test CPU efficiency under high load."""
        # Create mock provider and client
        provider = DeltaExchangeInstrumentProvider(
            client=MagicMock(),
            config=self.config,
            clock=self.clock,
        )
        
        data_client = DeltaExchangeDataClient(
            loop=self.loop,
            client=MagicMock(),
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            instrument_provider=provider,
            config=self.config,
        )
        
        self._setup_performance_monitoring(data_client)
        
        # Monitor CPU usage
        process = psutil.Process()
        cpu_samples = []
        
        # Start CPU monitoring
        process.cpu_percent()  # Initialize
        
        # Run CPU-intensive test
        start_time = time.time()
        message_count = 10000
        base_timestamp = int(time.time() * 1_000_000)
        
        for i in range(message_count):
            timestamp = base_timestamp + i * 1000
            instrument = self.test_instruments[i % len(self.test_instruments)]
            
            # Mix of message types to test different code paths
            if i % 4 == 0:
                message = self._create_mock_ticker_message(instrument.symbol.value, timestamp)
            elif i % 4 == 1:
                message = self._create_mock_trade_message(instrument.symbol.value, timestamp)
            else:
                message = self._create_mock_orderbook_message(instrument.symbol.value, timestamp)
            
            await data_client._handle_ws_message(json.dumps(message))
            
            # Sample CPU usage periodically
            if i % 1000 == 0:
                cpu_percent = process.cpu_percent()
                cpu_samples.append(cpu_percent)
        
        end_time = time.time()
        final_cpu = process.cpu_percent()
        
        # Calculate metrics
        total_time = end_time - start_time
        throughput = message_count / total_time
        avg_cpu = mean(cpu_samples) if cpu_samples else final_cpu
        
        # CPU efficiency assertions
        assert avg_cpu < 80.0, f"CPU usage too high: {avg_cpu:.1f}%"
        assert throughput > 1000, f"CPU-bound throughput too low: {throughput:.2f} msg/s"
        
        print(f"CPU efficiency results:")
        print(f"  Messages processed: {message_count}")
        print(f"  Processing time: {total_time:.2f}s")
        print(f"  Throughput: {throughput:.2f} messages/second")
        print(f"  Average CPU usage: {avg_cpu:.1f}%")
