from typing import Any

import numpy as np
import pandas as pd



BAR_PRICES: tuple[str, str, str, str]
BAR_COLUMNS: tuple[str, str, str, str, str]

def preprocess_bar_data(data: pd.DataFrame, is_raw: bool) -> pd.DataFrame:
    """
    Preprocess financial bar data to a standardized format.

    Ensures the DataFrame index is labeled as "timestamp", converts the index to UTC, removes time zone awareness,
    drops rows with NaN values in critical columns, and optionally scales the data.

    Parameters
    ----------
        data : pd.DataFrame
            The input DataFrame containing financial bar data.
        is_raw : bool
            A flag to determine whether the data should be scaled. If True, scales the data back by FIXED_SCALAR.

    Returns
    -------
        pd.DataFrame: The preprocessed DataFrame with a cleaned and standardized structure.

    """
    ...
def calculate_bar_price_offsets(num_records: int, timestamp_is_close: bool, offset_interval_ms: int, random_seed: int | None = None) -> dict[str, Any]:
    """
    Calculate and potentially randomize the time offsets for bar prices based on the closeness of the timestamp.

    Parameters
    ----------
        num_records : int
            The number of records for which offsets are to be generated.
        timestamp_is_close : bool
            A flag indicating whether the timestamp is close to the trading time.
        offset_interval_ms : int
            The offset interval in milliseconds to be applied.
        random_seed : Optional[int]
            The seed for random number generation to ensure reproducibility.

    Returns
    -------
        dict: A dictionary with arrays of offsets for open, high, low, and close prices. If random_seed is provided,
              high and low offsets are randomized.
    """
    ...
def calculate_volume_quarter(volume: np.ndarray, precision: int, size_increment: float) -> np.ndarray:
    """
    Convert raw volume data to quarter precision.

    Parameters
    ----------
    volume : np.ndarray
        An array of volume data to be processed.
    precision : int
        The decimal precision to which the volume data is rounded.

    Returns
    -------
    np.ndarray
        The volume data adjusted to quarter precision.

    """
    ...
def align_bid_ask_bar_data(bid_data: pd.DataFrame, ask_data: pd.DataFrame) -> pd.DataFrame:
    """
    Merge bid and ask data into a single DataFrame with prefixed column names.

    Parameters
    ----------
    bid_data : pd.DataFrame
        The DataFrame containing bid data.
    ask_data : pd.DataFrame
        The DataFrame containing ask data.

    Returns
    pd.DataFrame
        A merged DataFrame with columns prefixed by 'bid_' for bid data and 'ask_' for ask data, joined on their indexes.

    """
    ...
def prepare_event_and_init_timestamps(index: pd.DatetimeIndex, ts_init_delta: int) -> tuple[np.ndarray, np.ndarray]: ...

class OrderBookDeltaDataWrangler:
    """
    Provides a means of building lists of Nautilus `OrderBookDelta` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.

    """
    instrument: Instrument
    def __init__(self, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, ts_init_delta: int = 0, is_raw: bool = False) -> list[OrderBookDelta]:
        """
        Process the given order book dataset into Nautilus `OrderBookDelta` objects.

        Parameters
        ----------
        data : pd.DataFrame
            The data to process.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point values.

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        ...
    def _build_delta(
        self,
        action: BookAction,
        side: OrderSide,
        price: float,
        size: float,
        order_id: int,
        flags: int,
        sequence: int,
        ts_event: int,
        ts_init: int,
    ) -> OrderBookDelta: ...

class QuoteTickDataWrangler:
    """
    Provides a means of building lists of Nautilus `QuoteTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.
    """
    instrument: Instrument
    def __init__(self, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, default_volume: float = 1_000_000.0, ts_init_delta: int = 0) -> list[QuoteTick]:
        """
        Process the given tick dataset into Nautilus `QuoteTick` objects.

        Expects columns ['bid_price', 'ask_price'] with 'timestamp' index.
        Note: The 'bid_size' and 'ask_size' columns are optional, will then use
        the `default_volume`.

        Parameters
        ----------
        data : pd.DataFrame
            The tick data to process.
        default_volume : float
            The default volume for each tick (if not provided).
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system. Cannot be negative.

        Returns
        -------
        list[QuoteTick]

        """
        ...
    def process_bar_data(self, bid_data: pd.DataFrame, ask_data: pd.DataFrame, default_volume: float = 1_000_000.0, ts_init_delta: int = 0, offset_interval_ms: int = 100, timestamp_is_close: bool = True, random_seed: int | None = None, is_raw: bool = False, sort_data: bool = True) -> list[QuoteTick]:
        """
        Process the given bar datasets into Nautilus `QuoteTick` objects.

        Expects columns ['open', 'high', 'low', 'close', 'volume'] with 'timestamp' index.
        Note: The 'volume' column is optional, will then use the `default_volume`.

        Parameters
        ----------
        bid_data : pd.DataFrame
            The bid bar data.
        ask_data : pd.DataFrame
            The ask bar data.
        default_volume : float
            The volume per tick if not available from the data.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        offset_interval_ms : int, default 100
            The number of milliseconds to offset each tick for the bar timestamps.
            If `timestamp_is_close` then will use negative offsets,
            otherwise will use positive offsets (see also `timestamp_is_close`).
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is ``None`` then won't shuffle.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point values.
        timestamp_is_close : bool, default True
            If bar timestamps are at the close.
            If True, then open, high, low timestamps are offset before the close timestamp.
            If False, then high, low, close timestamps are offset after the open timestamp.
        sort_data : bool, default True
            If the data should be sorted by timestamp.

        """
        ...
    def _create_quote_ticks_array(self, merged_data: Any, is_raw: bool, instrument: Instrument, offsets: dict[str, Any], ts_init_delta: int) -> np.ndarray: ...
    def _build_tick(
        self,
        bid: float,
        ask: float,
        bid_size: float,
        ask_size: float,
        ts_event: int,
        ts_init: int,
    ) -> QuoteTick: ...

class TradeTickDataWrangler:
    """
    Provides a means of building lists of Nautilus `TradeTick` objects.

    Parameters
    ----------
    instrument : Instrument
        The instrument for the data wrangler.
    """
    instrument: Instrument
    def __init__(self, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, ts_init_delta: int = 0, is_raw: bool = False) -> list[TradeTick]:
        """
        Process the given trade tick dataset into Nautilus `TradeTick` objects.

        Parameters
        ----------
        data : pd.DataFrame
            The data to process.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point values.

        Returns
        -------
        list[TradeTick]

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        ...
    def process_bar_data(self, data: pd.DataFrame, ts_init_delta: int = 0, offset_interval_ms: int = 100, timestamp_is_close: bool = True, random_seed: int | None = None, is_raw: bool = False, sort_data: bool = True) -> list[TradeTick]:
        """
        Process the given bar datasets into Nautilus `TradeTick` objects.

        Expects columns ['open', 'high', 'low', 'close', 'volume'] with 'timestamp' index.
        Note: The 'volume' column is optional, will then use the `default_volume`.

        Parameters
        ----------
        data : pd.DataFrame
            The trade bar data.
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.
        offset_interval_ms : int, default 100
            The number of milliseconds to offset each tick for the bar timestamps.
            If `timestamp_is_close` then will use negative offsets,
            otherwise will use positive offsets (see also `timestamp_is_close`).
        random_seed : int, optional
            The random seed for shuffling order of high and low ticks from bar
            data. If random_seed is ``None`` then won't shuffle.
        is_raw : bool, default False
            If the data is scaled to Nautilus fixed-point.
        timestamp_is_close : bool, default True
            If bar timestamps are at the close.
            If True, then open, high, low timestamps are offset before the close timestamp.
            If False, then high, low, close timestamps are offset after the open timestamp.
        sort_data : bool, default True
            If the data should be sorted by timestamp.

        Returns
        -------
        list[TradeTick]

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        ...
    def _create_trade_ticks_array(self, records: Any, offsets: dict[str, Any]) -> np.ndarray: ...
    def _create_side_if_not_exist(self, data: pd.DataFrame) -> Any: ...
    def _build_tick(
        self,
        price: float,
        size: float,
        aggressor_side: AggressorSide,
        trade_id: str,
        ts_event: int,
        ts_init: int,
    ) -> TradeTick: ...

class BarDataWrangler:
    """
    Provides a means of building lists of Nautilus `Bar` objects.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the wrangler.
    instrument : Instrument
        The instrument for the wrangler.
    """
    bar_type: BarType
    instrument: Instrument
    def __init__(self, bar_type: BarType, instrument: Instrument) -> None: ...
    def process(self, data: pd.DataFrame, default_volume: float = 1_000_000.0, ts_init_delta: int = 0) -> list[Bar]:
        """
        Process the given bar dataset into Nautilus `Bar` objects.

        Expects columns ['open', 'high', 'low', 'close', 'volume'] with 'timestamp' index.
        Note: The 'volume' column is optional, if one does not exist then will use the `default_volume`.

        Parameters
        ----------
        data : pd.DataFrame
            The data to process.
        default_volume : float
            The default volume for each bar (if not provided).
        ts_init_delta : int
            The difference in nanoseconds between the data timestamps and the
            `ts_init` value. Can be used to represent/simulate latency between
            the data source and the Nautilus system.

        Returns
        -------
        list[Bar]

        Raises
        ------
        ValueError
            If `data` is empty.

        """
        ...
    def _build_bar(self, values: memoryview, ts_event: int, ts_init: int) -> Bar: ...

