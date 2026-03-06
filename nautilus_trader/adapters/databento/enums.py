from enum import Enum


class DatabentoSchema(Enum):
    """
    Represents a Databento schema.
    """

    MBO = "mbo"
    MBP_1 = "mbp-1"
    MBP_10 = "mbp-10"
    BBO_1S = "bbo-1s"
    BBO_1M = "bbo-1m"
    CMBP_1 = "cmbp-1"
    CBBO_1S = "cbbo-1s"
    CBBO_1M = "cbbo-1m"
    TCBBO = "tcbbo"
    TBBO = "tbbo"
    TRADES = "trades"
    OHLCV_1S = "ohlcv-1s"
    OHLCV_1M = "ohlcv-1m"
    OHLCV_1H = "ohlcv-1h"
    OHLCV_1D = "ohlcv-1d"
    OHLCV_EOD = "ohlcv-eod"
    DEFINITION = "definition"
    IMBALANCE = "imbalance"
    STATISTICS = "statistics"
    STATUS = "status"
