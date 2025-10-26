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

import logging
import os
import time
import warnings
from datetime import datetime
from decimal import Decimal
from pathlib import Path

import pandas as pd

from nautilus_trader.adapters.bybit import BYBIT
from nautilus_trader.adapters.bybit import BybitDataClientConfig
from nautilus_trader.adapters.bybit import BybitProductType
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import InstrumentClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.strategy import Strategy


warnings.simplefilter(action="ignore", category=FutureWarning)


class BybitOptionsDataCollectorConfig(StrategyConfig, frozen=True):
    """
    Configuration for the Bybit Options Data Collector Strategy.
    """

    spot_instrument_id: InstrumentId  # Spot instrument ID (required)
    underlying_asset: str = "BTC"  # The underlying asset (e.g., "BTC")
    options_depth: int = 25  # Options support 25 or 100 levels
    spot_depth: int = 50  # Spot supports 1, 50, or 200 levels
    batch_size: int = 1000  # Number of records to batch before writing to parquet
    data_dir: str = "data"  # Directory to store parquet files
    log_interval: float = 60.0  # Log and save data every N seconds
    verbose_logging: bool = True  # Enable verbose logging for debugging
    save_logs: bool = True  # Save logs to file
    log_level: str = "INFO"  # Log level for file logging


class BybitOptionsDataCollector(Strategy):
    """
    A strategy that discovers all available options for a given underlying asset,
    subscribes to their data, and stores them in parquet files.
    """

    def __init__(self, config: BybitOptionsDataCollectorConfig) -> None:
        super().__init__(config)

        # Configuration
        self.underlying_asset = config.underlying_asset
        self.batch_size = config.batch_size
        self.data_dir = config.data_dir
        self.log_interval = config.log_interval

        # Create hierarchical data directory structure
        self.base_data_dir = os.path.join(self.data_dir, self.underlying_asset, "USDT")
        self.spot_data_dir = os.path.join(self.base_data_dir, "spot")
        self.options_data_dir = os.path.join(self.base_data_dir, "options")

        # Create directories
        os.makedirs(self.spot_data_dir, exist_ok=True)
        os.makedirs(self.options_data_dir, exist_ok=True)

        # Setup file logging if enabled
        self._setup_file_logging()

        # Data storage
        self.quote_ticks_data: dict[str, list[dict]] = {}
        self.order_book_deltas_data: dict[str, list[dict]] = {}

        # File paths for each instrument - will be created dynamically
        self.quote_ticks_files: dict[str, str] = {}
        self.order_book_deltas_files: dict[str, str] = {}

        # Order books for each instrument
        self.books: dict[str, OrderBook] = {}

        # Counters
        self.quote_count = 0
        self.delta_count = 0
        self.spot_quote_count = 0
        self.last_log_time = 0.0
        self.last_spot_price: Decimal | None = None

        # Connection monitoring
        self.last_data_time = time.time()
        self.connection_warnings = 0

        # Per-instrument counters for logging
        self.instrument_quote_counts: dict[str, int] = {}
        self.instrument_delta_counts: dict[str, int] = {}

        # Discovered options
        self.discovered_options: list[InstrumentId] = []

        # File logging attributes
        self.file_handler: logging.FileHandler | None = None
        self.log_filepath: str | None = None

        # Initialize data storage for spot instrument
        self._initialize_spot_data_storage()

    def _setup_file_logging(self) -> None:
        """
        Set up file logging to save logs alongside data.
        """
        # Skip custom file logging setup - use NautilusTrader's built-in logging
        if not self.config.save_logs:
            return

        # Create logs directory for reference
        logs_dir = os.path.join(self.base_data_dir, "logs")
        os.makedirs(logs_dir, exist_ok=True)

        # Log that file logging is handled by NautilusTrader kernel
        self.log.info(f"File logging directory: {logs_dir}")
        self.log.info("File logging is handled by NautilusTrader kernel configuration")

    def _initialize_spot_data_storage(self) -> None:
        """
        Initialize data storage for spot instrument.
        """
        # Initialize spot instrument
        spot_key = str(self.config.spot_instrument_id)
        self.quote_ticks_data[spot_key] = []
        self.order_book_deltas_data[spot_key] = []
        self.instrument_quote_counts[spot_key] = 0
        self.instrument_delta_counts[spot_key] = 0

        # Create file paths for spot instrument (in spot folder)
        spot_name = spot_key.replace(".", "_")
        self.quote_ticks_files[spot_key] = os.path.join(
            self.spot_data_dir,
            f"{spot_name}_quote.parquet",
        )
        self.order_book_deltas_files[spot_key] = os.path.join(
            self.spot_data_dir,
            f"{spot_name}_orderbook.parquet",
        )

        # Log file creation for spot
        self.log.info("Data will be saved to the following directory structure:")
        self.log.info(f"Base directory: {self.base_data_dir}")
        self.log.info(f"Spot quote ticks: {self.quote_ticks_files[spot_key]}")
        self.log.info(f"Spot order book deltas: {self.order_book_deltas_files[spot_key]}")

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.log.info("Starting Bybit Options Data Collector Strategy")

        # Discover all available options for the underlying asset
        self._discover_options()

        # Initialize data storage for discovered options
        self._initialize_options_data_storage()

        # Validate and get instruments
        if not self._validate_instruments():
            return

        # Initialize order books
        self._initialize_order_books()

        # Subscribe to data streams
        self._subscribe_to_data_streams()

    def _discover_options(self) -> None:
        """
        Discover all available options for the underlying asset.
        """
        self.log.info(f"Discovering all available {self.underlying_asset} options...")

        # Get and filter options from cache instead of client
        all_instruments = self.cache.instruments()
        options = [
            instrument
            for instrument in all_instruments
            if (
                str(instrument.symbol).startswith(self.underlying_asset)
                and instrument.instrument_class
                == InstrumentClass.OPTION  # Changed from InstrumentType.OPTION
            )
        ]

        # Group by expiry and subscribe
        expiry_groups: dict[str, list[Instrument]] = {}
        for option in options:
            # Convert expiration_ns to a readable date format for grouping
            from nautilus_trader.core.datetime import unix_nanos_to_dt

            expiry_dt = unix_nanos_to_dt(option.expiration_ns)
            expiry = expiry_dt.strftime("%d%b%y").upper()  # e.g., "02AUG25"

            if expiry not in expiry_groups:
                expiry_groups[expiry] = []
            expiry_groups[expiry].append(option)

        # Store discovered options for later use
        self.discovered_options = [option.id for option in options]

        self.log.info(f"Discovered {len(options)} options across {len(expiry_groups)} expiries")

    def _initialize_options_data_storage(self) -> None:
        """
        Initialize data storage for all discovered options.
        """
        for instrument_id in self.discovered_options:
            instrument_key = str(instrument_id)
            self.quote_ticks_data[instrument_key] = []
            self.order_book_deltas_data[instrument_key] = []
            self.instrument_quote_counts[instrument_key] = 0
            self.instrument_delta_counts[instrument_key] = 0

            # Create file paths for options instruments (in options folder)
            instrument_name = instrument_key.replace(".", "_")
            self.quote_ticks_files[instrument_key] = os.path.join(
                self.options_data_dir,
                f"{instrument_name}_quote.parquet",
            )
            self.order_book_deltas_files[instrument_key] = os.path.join(
                self.options_data_dir,
                f"{instrument_name}_orderbook.parquet",
            )

        # Log summary of options data files
        options_count = len(
            [k for k in self.quote_ticks_files.keys() if k != str(self.config.spot_instrument_id)],
        )
        self.log.info(f"Will create data files for {options_count} options instruments")

        # Show detailed file paths only if verbose logging is enabled
        if self.config.verbose_logging:
            self.log.info("Options data files:")
            for instrument_key in self.quote_ticks_files:
                if instrument_key != str(self.config.spot_instrument_id):
                    self.log.info(f"  Quote ticks: {self.quote_ticks_files[instrument_key]}")
                    self.log.info(
                        f"  Order book deltas: {self.order_book_deltas_files[instrument_key]}",
                    )

    def _validate_instruments(self) -> bool:
        """
        Validate that all instruments exist in cache.
        """
        # Validate spot instrument
        self.spot_instrument = self.cache.instrument(self.config.spot_instrument_id)
        if self.spot_instrument is None:
            self.log.error(f"Could not find spot instrument for {self.config.spot_instrument_id}")
            self.stop()
            return False

        self.log.info(f"Found spot instrument: {self.spot_instrument}")

        # Validate options instruments
        self.instruments = {}
        valid_options = []

        for instrument_id in self.discovered_options:
            instrument = self.cache.instrument(instrument_id)
            if instrument is None:
                self.log.warning(f"Could not find instrument for {instrument_id}")
                continue

            # Additional validation for options instruments
            if instrument.instrument_class != InstrumentClass.OPTION:
                self.log.warning(
                    f"Instrument {instrument_id} is not an option: {instrument.instrument_class}",
                )
                continue

            # Check if instrument is active/tradeable
            if hasattr(instrument, "is_active") and not instrument.is_active:
                self.log.warning(f"Instrument {instrument_id} is not active")
                continue

            self.instruments[str(instrument_id)] = instrument
            valid_options.append(instrument_id)

        # Update discovered options to only include valid ones
        self.discovered_options = valid_options
        self.log.info(f"Validated {len(self.discovered_options)} options instruments")

        if len(self.discovered_options) == 0:
            self.log.error("No valid options instruments found - stopping strategy")
            self.stop()
            return False

        return True

    def _initialize_order_books(self) -> None:
        """
        Initialize order books for all instruments (options and spot).
        """
        # Initialize order books for options instruments
        for instrument_id in self.discovered_options:
            instrument_key = str(instrument_id)
            self.books[instrument_key] = OrderBook(
                instrument_id=instrument_id,
                book_type=BookType.L2_MBP,
            )

        # Initialize order book for spot instrument
        spot_key = str(self.config.spot_instrument_id)
        self.books[spot_key] = OrderBook(
            instrument_id=self.config.spot_instrument_id,
            book_type=BookType.L2_MBP,
        )

    def _subscribe_to_data_streams(self) -> None:
        """
        Subscribe to all data streams.
        """
        # Subscribe to quote ticks for all options instruments FIRST
        for instrument_id in self.discovered_options:
            self.subscribe_quote_ticks(instrument_id=instrument_id)

        # Subscribe to spot quote ticks
        self.subscribe_quote_ticks(instrument_id=self.config.spot_instrument_id)

        # Subscribe to order book deltas for all options instruments AFTER quotes
        for instrument_id in self.discovered_options:
            self.subscribe_order_book_deltas(
                instrument_id=instrument_id,
                book_type=BookType.L2_MBP,
                depth=self.config.options_depth,
            )

        # Subscribe to spot order book deltas
        self.subscribe_order_book_deltas(
            instrument_id=self.config.spot_instrument_id,
            book_type=BookType.L2_MBP,
            depth=self.config.spot_depth,
        )

        # Get expiry groups for detailed logging
        expiry_groups = self._get_expiry_groups()

        # Log subscription summary with maturity breakdown
        self.log.info(f"Subscribed to {len(self.discovered_options)} options and 1 spot instrument")
        self.log.info(
            f"Monitoring {len(self.discovered_options)} options across {len(expiry_groups)} maturities:",
        )

        # Log each maturity with option count
        for expiry in sorted(expiry_groups.keys()):
            count = expiry_groups[expiry]
            self.log.info(f"  {expiry}: {count} options")

        self.log.info("Data collection started - waiting for market data...")

    def _get_expiry_groups(self) -> dict[str, int]:
        """
        Get expiry groups for logging summary.
        """
        expiry_groups = {}
        for instrument_id in self.discovered_options:
            symbol = str(instrument_id.symbol)
            parts = symbol.split("-")
            if len(parts) >= 4:
                expiry = parts[1]  # e.g., "02AUG25"
                if expiry not in expiry_groups:
                    expiry_groups[expiry] = 0
                expiry_groups[expiry] += 1
        return expiry_groups

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Handle incoming order book deltas.
        """
        # Update last data time
        self.last_data_time = time.time()

        instrument_key = str(deltas.instrument_id)

        if instrument_key not in self.books:
            self.log.error(f"No order book initialized for {instrument_key}")
            return

        # Apply deltas to maintain the order book
        self.books[instrument_key].apply_deltas(deltas)
        self.delta_count += 1
        self.instrument_delta_counts[instrument_key] += 1

        # Store delta data
        self._store_order_book_delta(deltas, instrument_key)

        # Check if we need to log and save data
        self._check_and_log_data()

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Handle incoming quote ticks (both options and spot).
        """
        # Update last data time
        self.last_data_time = time.time()

        instrument_key = str(tick.instrument_id)

        # Add validation for quote data
        if tick.bid_price.as_double() <= 0 or tick.ask_price.as_double() <= 0:
            self.log.warning(
                f"Invalid quote prices for {instrument_key}: bid={tick.bid_price}, ask={tick.ask_price}",
            )
            return

        if tick.bid_price.as_double() >= tick.ask_price.as_double():
            self.log.warning(
                f"Invalid quote spread for {instrument_key}: bid={tick.bid_price}, ask={tick.ask_price}",
            )
            return

        # Store quote tick data
        self._store_quote_tick(tick, instrument_key)

        # Update counters
        self._update_counters(tick, instrument_key)

        # Check if we need to log and save data
        self._check_and_log_data()

    def _store_order_book_delta(self, deltas: OrderBookDeltas, instrument_key: str) -> None:
        """
        Store order book delta data.
        """
        book = self.books[instrument_key]
        delta_data = {
            "timestamp": pd.Timestamp.now(),
            "instrument_id": str(deltas.instrument_id),
            "sequence": deltas.sequence,
            "delta_count": len(deltas.deltas),
            "best_bid": book.best_bid_price().as_double() if book.best_bid_price() else None,
            "best_ask": book.best_ask_price().as_double() if book.best_ask_price() else None,
            "bid_size": book.best_bid_size().as_double() if book.best_bid_size() else None,
            "ask_size": book.best_ask_size().as_double() if book.best_ask_size() else None,
        }

        self.order_book_deltas_data[instrument_key].append(delta_data)

    def _store_quote_tick(self, tick: QuoteTick, instrument_key: str) -> None:
        """
        Store quote tick data.
        """
        quote_data = {
            "timestamp": pd.Timestamp.now(),
            "instrument_id": str(tick.instrument_id),
            "bid_price": tick.bid_price.as_double(),
            "ask_price": tick.ask_price.as_double(),
            "bid_size": tick.bid_size.as_double(),
            "ask_size": tick.ask_size.as_double(),
            "ts_event": tick.ts_event,
            "ts_init": tick.ts_init,
        }

        self.quote_ticks_data[instrument_key].append(quote_data)

    def _update_counters(self, tick: QuoteTick, instrument_key: str) -> None:
        """
        Update counters for the instrument.
        """
        if instrument_key == str(self.config.spot_instrument_id):
            self.spot_quote_count += 1
            self.last_spot_price = (tick.bid_price + tick.ask_price) / 2  # Mid price
        else:
            self.quote_count += 1

        self.instrument_quote_counts[instrument_key] += 1

    def _check_and_log_data(self) -> None:
        """
        Check if it's time to log and save data.
        """
        current_time = time.time()

        # Check for data timeout (no data for 2 minutes)
        if current_time - self.last_data_time > 120:  # 2 minutes
            self.connection_warnings += 1
            if self.connection_warnings <= 3:  # Only warn first 3 times
                self.log.warning(
                    f"No data received for {int(current_time - self.last_data_time)} seconds - possible connection issue",
                )
            elif self.connection_warnings == 4:
                self.log.error("Multiple connection warnings - consider restarting the strategy")
        else:
            # Reset warnings if we're getting data
            self.connection_warnings = 0

        if current_time - self.last_log_time >= self.log_interval:
            self._log_and_save_data()
            self.last_log_time = current_time

    def _log_and_save_data(self) -> None:
        """
        Log statistics and save all data to parquet files.
        """
        # Log statistics for each instrument
        self.log.info(f"=== {self.log_interval} SECOND UPDATE ===")

        # Calculate totals
        options_quote_total = 0
        options_delta_total = 0
        active_options = 0

        for instrument_key in self.instrument_quote_counts:
            if instrument_key != str(self.config.spot_instrument_id):
                quote_count = self.instrument_quote_counts[instrument_key]
                delta_count = self.instrument_delta_counts.get(instrument_key, 0)
                options_quote_total += quote_count
                options_delta_total += delta_count
                if quote_count > 0 or delta_count > 0:
                    active_options += 1

        # Log spot instrument
        spot_key = str(self.config.spot_instrument_id)
        spot_quote_count = self.instrument_quote_counts.get(spot_key, 0)
        spot_delta_count = self.instrument_delta_counts.get(spot_key, 0)

        # Log summary
        total_quotes = options_quote_total + spot_quote_count
        total_deltas = options_delta_total + spot_delta_count

        # Get current expiry groups for status
        expiry_groups = self._get_expiry_groups()

        self.log.info(
            f"Active instruments: {active_options}/{len(self.discovered_options)} options, 1/1 spot",
        )
        self.log.info(
            f"Monitoring {len(expiry_groups)} maturities: {', '.join(sorted(expiry_groups.keys()))}",
        )
        self.log.info(f"Data received: {total_quotes} quotes, {total_deltas} deltas")
        self.log.info(f"  Options: {options_quote_total} quotes, {options_delta_total} deltas")
        self.log.info(f"  Spot: {spot_quote_count} quotes, {spot_delta_count} deltas")

        # Save all data to parquet files
        self._save_all_data_to_parquet()

        # Reset counters for next interval
        self._reset_counters()

    def _save_all_data_to_parquet(self) -> None:
        """
        Save all accumulated data to parquet files.
        """
        # Count data being saved
        total_quotes_saved = 0
        total_deltas_saved = 0
        instruments_with_quotes = 0
        instruments_with_deltas = 0

        # Save quote ticks data for each instrument
        for instrument_key, data in self.quote_ticks_data.items():
            if data:
                filepath = self.quote_ticks_files[instrument_key]
                self._append_to_parquet_file(data, filepath, "quote_ticks", instrument_key)
                total_quotes_saved += len(data)
                instruments_with_quotes += 1
                # Clear the data after saving to prevent duplicate appending
                self.quote_ticks_data[instrument_key].clear()

        # Save order book deltas data for each instrument
        for instrument_key, data in self.order_book_deltas_data.items():
            if data:
                filepath = self.order_book_deltas_files[instrument_key]
                self._append_to_parquet_file(data, filepath, "order_book_deltas", instrument_key)
                total_deltas_saved += len(data)
                instruments_with_deltas += 1
                # Clear the data after saving to prevent duplicate appending
                self.order_book_deltas_data[instrument_key].clear()

        # Log data saving summary
        if total_quotes_saved > 0 or total_deltas_saved > 0:
            self.log.info(
                f"Data saved: {total_quotes_saved} quotes ({instruments_with_quotes} instruments), "
                f"{total_deltas_saved} deltas ({instruments_with_deltas} instruments)",
            )

    def _append_to_parquet_file(
        self,
        data: list[dict],
        filepath: str,
        data_type: str,
        instrument_key: str,
    ) -> None:
        """
        Append data to a parquet file.
        """
        if not data:
            return

        # Ensure directory exists
        Path(filepath).parent.mkdir(parents=True, exist_ok=True)

        # Convert to DataFrame
        df = pd.DataFrame(data)

        try:
            if df.empty:
                return

            # If existing file exists and has data, read and concat
            if Path(filepath).exists():
                try:
                    existing_df = pd.read_parquet(filepath)
                    if existing_df.empty:
                        combined_df = df
                    else:
                        # Align columns safely
                        all_columns = sorted(set(existing_df.columns) | set(df.columns))
                        for col in all_columns:
                            if col not in existing_df:
                                existing_df[col] = pd.NA
                            if col not in df:
                                df[col] = pd.NA
                        combined_df = pd.concat(
                            [existing_df[all_columns], df[all_columns]],
                            ignore_index=True,
                        )
                except Exception as e:
                    self.log.error(f"Failed to read existing parquet file: {e}")
                    combined_df = df
            else:
                combined_df = df

            # Save to file
            combined_df.to_parquet(filepath, index=False)

            self.log.debug(f"Saved {len(df)} {data_type} records to {filepath}")

        except Exception as e:
            self.log.error(f"Error saving {data_type} to {filepath}: {e}")

    def _reset_counters(self) -> None:
        """
        Reset counters for next interval.
        """
        for key in self.instrument_quote_counts:
            self.instrument_quote_counts[key] = 0
        for key in self.instrument_delta_counts:
            self.instrument_delta_counts[key] = 0

    def on_stop(self) -> None:
        """
        Actions to be performed on strategy stop.
        """
        # Calculate final statistics
        active_options = 0
        total_options_quotes = 0
        total_options_deltas = 0

        for instrument_key in self.instrument_quote_counts:
            if instrument_key != str(self.config.spot_instrument_id):
                quote_count = self.instrument_quote_counts[instrument_key]
                delta_count = self.instrument_delta_counts.get(instrument_key, 0)
                total_options_quotes += quote_count
                total_options_deltas += delta_count
                if quote_count > 0 or delta_count > 0:
                    active_options += 1

        # Log final statistics
        self.log.info("=== FINAL STATISTICS ===")
        self.log.info(
            f"Active instruments: {active_options}/{len(self.discovered_options)} options, 1/1 spot",
        )
        self.log.info("Total data processed:")
        self.log.info(f"  Options: {total_options_quotes} quotes, {total_options_deltas} deltas")
        self.log.info(
            f"  Spot: {self.spot_quote_count} quotes, {self.instrument_delta_counts.get(str(self.config.spot_instrument_id), 0)} deltas",
        )

        # Write any remaining data to parquet
        self._save_all_data_to_parquet()

        self.log.info(
            f"Strategy stopped. "
            f"Processed {self.delta_count} order book deltas, "
            f"{self.quote_count} options quote ticks, and "
            f"{self.spot_quote_count} spot quote ticks.",
        )

        # Cleanup file logging if enabled
        if hasattr(self, "file_handler") and self.file_handler:
            self.log.info(f"Logs saved to: {self.log_filepath}")
            self.file_handler.close()
            self.log.removeHandler(self.file_handler)

    def get_log_filepath(self) -> str | None:
        """
        Get the current log file path.

        Returns
        -------
        Optional[str]
            The log file path if file logging is enabled, None otherwise

        """
        return getattr(self, "log_filepath", None)

    def rotate_log_file(self) -> str | None:
        """
        Rotate the current log file and create a new one. Useful for long-running
        sessions to manage log file sizes.

        Returns
        -------
        Optional[str]
            The new log file path if successful, None otherwise

        """
        if not hasattr(self, "file_handler") or not self.file_handler:
            return None

        try:
            # Close current handler
            self.file_handler.close()
            self.log.removeHandler(self.file_handler)

            # Create new log filename with timestamp
            timestamp = datetime.now().strftime("%Y%m%d_%H%M%S")
            logs_dir = os.path.join(self.base_data_dir, "logs")
            log_filename = f"bybit_options_collector_{self.underlying_asset}_{timestamp}.log"
            new_log_filepath = os.path.join(logs_dir, log_filename)

            # Create new file handler
            new_file_handler = logging.FileHandler(new_log_filepath)
            new_file_handler.setLevel(getattr(logging, self.config.log_level))

            # Create formatter
            formatter = logging.Formatter(
                "%(asctime)s - %(name)s - %(levelname)s - %(message)s",
                datefmt="%Y-%m-%d %H:%M:%S",
            )
            new_file_handler.setFormatter(formatter)

            # Add new handler to logger
            self.log.addHandler(new_file_handler)

            # Update stored references
            self.file_handler = new_file_handler
            old_log_filepath = self.log_filepath
            self.log_filepath = new_log_filepath

            # Log the rotation
            self.log.info(f"Log file rotated: {old_log_filepath} -> {new_log_filepath}")

            return new_log_filepath

        except Exception as e:
            self.log.error(f"Failed to rotate log file: {e}")
            return None


def main():
    """
    Run the Bybit options data collector.
    """
    # Configuration for Bybit options and spot
    product_types = [BybitProductType.OPTION, BybitProductType.SPOT]

    # Create spot symbol for the underlying asset
    underlying = "BTC"
    spot_symbol = f"{underlying}USDT-SPOT.BYBIT"

    # Configure the trading node
    config_node = TradingNodeConfig(
        trader_id=TraderId("OPTIONS-COLLECTOR-001"),
        logging=LoggingConfig(
            log_level="INFO",
            log_level_file="INFO",  # Enable file logging
            log_directory="data/logs",  # Set log directory
            log_file_name="bybit_options_collector",  # Set log file name
            use_pyo3=True,
            log_colors=True,
        ),
        data_clients={
            BYBIT: BybitDataClientConfig(
                api_key=os.getenv("BYBIT_API_KEY"),
                api_secret=os.getenv("BYBIT_API_SECRET"),
                instrument_provider=InstrumentProviderConfig(
                    load_all=True,  # Load all instruments
                    filters={
                        "base_coin": underlying,  # Filter for BTC base coin only
                    },
                ),
                product_types=product_types,  # Load both options and spot
                testnet=False,  # Use mainnet
            ),
        },
        timeout_connection=30.0,
        timeout_reconciliation=10.0,
        timeout_portfolio=10.0,
        timeout_disconnection=10.0,
        timeout_post_stop=5.0,
    )

    # Instantiate the node
    node = TradingNode(config=config_node)

    # Configure the strategy
    strategy_config = BybitOptionsDataCollectorConfig(
        underlying_asset=underlying,  # Collect data for BTC options
        spot_instrument_id=InstrumentId.from_str(spot_symbol),
        options_depth=25,  # Use 25 levels for options
        spot_depth=50,  # Use 50 levels for spot
        batch_size=1000,  # Write to parquet every 1000 records
        data_dir="data",  # Store parquet files in 'data' directory
        log_interval=60.0,  # Log and save every 60 seconds
        save_logs=True,  # Save logs to file
        log_level="INFO",  # Log level for file logging
    )

    # Instantiate the strategy
    strategy = BybitOptionsDataCollector(config=strategy_config)

    # Add the strategy to the node
    node.trader.add_strategy(strategy)

    # Register the data client factory
    from nautilus_trader.adapters.bybit import BybitLiveDataClientFactory

    node.add_data_client_factory(BYBIT, BybitLiveDataClientFactory)

    # Build the node
    node.build()

    # Run the node
    if __name__ == "__main__":
        try:
            print("Starting Bybit Options Data Collector")
            print(f"Will discover and collect data for all {underlying} options")
            print("Press Ctrl+C to stop...")
            node.run()
        except KeyboardInterrupt:
            print("\nStopping...")
        finally:
            node.dispose()


if __name__ == "__main__":
    main()
