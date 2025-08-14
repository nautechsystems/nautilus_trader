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
The `indicator` subpackage provides a set of efficient indicators and analyzers.

These are classes which can be used for signal discovery and filtering. The idea is to
use the provided indicators as is, or as inspiration for a trader to implement their own
proprietary indicator algorithms with the platform.

"""

from nautilus_trader.indicators.averages import AdaptiveMovingAverage
from nautilus_trader.indicators.averages import DoubleExponentialMovingAverage
from nautilus_trader.indicators.averages import ExponentialMovingAverage
from nautilus_trader.indicators.averages import HullMovingAverage
from nautilus_trader.indicators.averages import MovingAverage
from nautilus_trader.indicators.averages import MovingAverageFactory
from nautilus_trader.indicators.averages import MovingAverageType
from nautilus_trader.indicators.averages import SimpleMovingAverage
from nautilus_trader.indicators.averages import VariableIndexDynamicAverage
from nautilus_trader.indicators.averages import WeightedMovingAverage
from nautilus_trader.indicators.averages import WilderMovingAverage
from nautilus_trader.indicators.base import Indicator
from nautilus_trader.indicators.fuzzy_candlesticks import FuzzyCandle
from nautilus_trader.indicators.fuzzy_candlesticks import FuzzyCandlesticks
from nautilus_trader.indicators.fuzzy_enums import CandleBodySize
from nautilus_trader.indicators.fuzzy_enums import CandleDirection
from nautilus_trader.indicators.fuzzy_enums import CandleSize
from nautilus_trader.indicators.fuzzy_enums import CandleWickSize
from nautilus_trader.indicators.momentum import ChandeMomentumOscillator
from nautilus_trader.indicators.momentum import CommodityChannelIndex
from nautilus_trader.indicators.momentum import EfficiencyRatio
from nautilus_trader.indicators.momentum import PsychologicalLine
from nautilus_trader.indicators.momentum import RateOfChange
from nautilus_trader.indicators.momentum import RelativeStrengthIndex
from nautilus_trader.indicators.momentum import RelativeVolatilityIndex
from nautilus_trader.indicators.momentum import Stochastics
from nautilus_trader.indicators.spread_analyzer import SpreadAnalyzer
from nautilus_trader.indicators.trend import ArcherMovingAveragesTrends
from nautilus_trader.indicators.trend import AroonOscillator
from nautilus_trader.indicators.trend import Bias
from nautilus_trader.indicators.trend import DirectionalMovement
from nautilus_trader.indicators.trend import LinearRegression
from nautilus_trader.indicators.trend import MovingAverageConvergenceDivergence
from nautilus_trader.indicators.trend import Swings
from nautilus_trader.indicators.volatility import AverageTrueRange
from nautilus_trader.indicators.volatility import BollingerBands
from nautilus_trader.indicators.volatility import DonchianChannel
from nautilus_trader.indicators.volatility import KeltnerChannel
from nautilus_trader.indicators.volatility import KeltnerPosition
from nautilus_trader.indicators.volatility import VerticalHorizontalFilter
from nautilus_trader.indicators.volatility import VolatilityRatio
from nautilus_trader.indicators.volume import KlingerVolumeOscillator
from nautilus_trader.indicators.volume import OnBalanceVolume
from nautilus_trader.indicators.volume import Pressure
from nautilus_trader.indicators.volume import VolumeWeightedAveragePrice


__all__ = [
    "AdaptiveMovingAverage",
    "ArcherMovingAveragesTrends",
    "AroonOscillator",
    "AverageTrueRange",
    "Bias",
    "BollingerBands",
    "CandleBodySize",
    "CandleDirection",
    "CandleSize",
    "CandleWickSize",
    "ChandeMomentumOscillator",
    "CommodityChannelIndex",
    "DirectionalMovement",
    "DonchianChannel",
    "DoubleExponentialMovingAverage",
    "EfficiencyRatio",
    "ExponentialMovingAverage",
    "FuzzyCandle",
    "FuzzyCandlesticks",
    "HullMovingAverage",
    "Indicator",
    "KeltnerChannel",
    "KeltnerPosition",
    "KlingerVolumeOscillator",
    "LinearRegression",
    "MovingAverage",
    "MovingAverageConvergenceDivergence",
    "MovingAverageFactory",
    "MovingAverageType",
    "OnBalanceVolume",
    "Pressure",
    "PsychologicalLine",
    "RateOfChange",
    "RelativeStrengthIndex",
    "RelativeVolatilityIndex",
    "SimpleMovingAverage",
    "SpreadAnalyzer",
    "Stochastics",
    "Swings",
    "VariableIndexDynamicAverage",
    "VerticalHorizontalFilter",
    "VolatilityRatio",
    "VolumeWeightedAveragePrice",
    "WeightedMovingAverage",
    "WilderMovingAverage",
]
