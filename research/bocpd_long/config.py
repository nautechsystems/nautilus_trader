"""
Settings of the BOCPD-trigger long-only strategy (bpd25 dollar bars). Every module reads its
parameters from here.
"""

from __future__ import annotations

from datetime import UTC
from datetime import datetime
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
BARS_DIR = ROOT / "binance_data" / "dollar_bars" / "bpd25"
OUT_DIR = ROOT / "binance_data" / "bocpd_long"

MIRROR_SUFFIX = "_M"
DAY_MS = 86_400_000

# Dev period; all 2026 data is sealed and never loaded
DEV_END_MS = int(datetime(2025, 7, 1, tzinfo=UTC).timestamp() * 1000)
SEAL_START_MS = int(datetime(2026, 1, 1, tzinfo=UTC).timestamp() * 1000)

# Volatility, barriers and labels
VOL_SPAN = 104  # EWMA span (bars) of the ~1-day log return
VOL_MIN_BARS = 104
BARRIER_MULT = 1.4  # horizontal barriers in units of sigma (also the CUSUM threshold)
VERTICAL_DAYS = 3

# Costs (taker fee per side, no slippage)
FEE = 0.0005

# Features (bar counts at 25 bars per day)
ZSCORE_BARS = 750  # ~30 days
MICRO_BARS = 125  # ~5 days
SHORT_BARS = 25  # ~1 day
SADF_WINDOW = 750
SADF_MIN_BARS = 125
SADF_STEP = 21
ENTROPY_BARS = 125
FFD_THRESHOLD = 1e-4
FFD_D_GRID = tuple(round(0.05 * i, 2) for i in range(21))
ADF_PVALUE = 0.05

# BOCPD on standardized bar returns, online-learned hazard
BOCPD_HAZARD_PRIOR_MEAN = 1 / 250
BOCPD_HAZARD_PRIOR_STRENGTH = 120.0
BOCPD_HAZARD_MEMORY = 4167.0
BOCPD_HAZARD_MIN = 1 / 4167
BOCPD_HAZARD_MAX = 1 / 12
BOCPD_MAX_RUN = 1250
BOCPD_RECENT_BARS = 25
BOCPD_SHIFT_BARS = 125
BOCPD_LAG = 25
BOCPD_LAG_WIDTH = 8
BOCPD_TRIGGER = 0.5

# Cross-validation
N_SPLITS = 5
EMBARGO_PCT = 0.01

# LightGBM (primary and meta), heavy regularization; rounds by purged-CV early stopping
LGBM_PARAMS = {
    "objective": "binary",
    "learning_rate": 0.02,
    "max_depth": 4,
    "num_leaves": 15,
    "min_data_in_leaf": 500,
    "lambda_l1": 10.0,
    "lambda_l2": 30.0,
    "feature_fraction": 0.5,
    "bagging_freq": 1,
    "min_gain_to_split": 0.0,
    "verbosity": -1,
    "seed": 42,
    "deterministic": True,
    "num_threads": 8,
}
MAX_ROUNDS = 2000
EARLY_STOPPING = 100

# Sizing: per-symbol cap and inverse-volatility scaling against the trailing median sigma
MAX_SYMBOL_WEIGHT = 0.05
SIGMA_REF_DAYS = 30
