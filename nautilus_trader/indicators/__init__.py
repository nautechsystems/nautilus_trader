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
# from nautilus_trader.core.nautilus_pyo3 import AdaptiveMovingAverage
# from nautilus_trader.core.nautilus_pyo3 import ArcherMovingAveragesTrends
# from nautilus_trader.core.nautilus_pyo3 import AroonOscillator
# from nautilus_trader.core.nautilus_pyo3 import AverageTrueRange
# from nautilus_trader.core.nautilus_pyo3 import Bias
# from nautilus_trader.core.nautilus_pyo3 import BollingerBands
# from nautilus_trader.core.nautilus_pyo3 import BookImbalanceRatio
# from nautilus_trader.core.nautilus_pyo3 import CandleBodySize
# from nautilus_trader.core.nautilus_pyo3 import CandleDirection
# from nautilus_trader.core.nautilus_pyo3 import CandleSize
# from nautilus_trader.core.nautilus_pyo3 import CandleWickSize
# from nautilus_trader.core.nautilus_pyo3 import ChandeMomentumOscillator
# from nautilus_trader.core.nautilus_pyo3 import CommodityChannelIndex
# from nautilus_trader.core.nautilus_pyo3 import DonchianChannel
# from nautilus_trader.core.nautilus_pyo3 import DoubleExponentialMovingAverage
# from nautilus_trader.core.nautilus_pyo3 import EfficiencyRatio
# from nautilus_trader.core.nautilus_pyo3 import ExponentialMovingAverage
# from nautilus_trader.core.nautilus_pyo3 import FuzzyCandle
# from nautilus_trader.core.nautilus_pyo3 import FuzzyCandlesticks
# from nautilus_trader.core.nautilus_pyo3 import HullMovingAverage
# from nautilus_trader.core.nautilus_pyo3 import KeltnerChannel
# from nautilus_trader.core.nautilus_pyo3 import KeltnerPosition
# from nautilus_trader.core.nautilus_pyo3 import KlingerVolumeOscillator
# from nautilus_trader.core.nautilus_pyo3 import LinearRegression
# from nautilus_trader.core.nautilus_pyo3 import MovingAverageConvergenceDivergence
# from nautilus_trader.core.nautilus_pyo3 import MovingAverageType
# from nautilus_trader.core.nautilus_pyo3 import OnBalanceVolume
# from nautilus_trader.core.nautilus_pyo3 import Pressure
# from nautilus_trader.core.nautilus_pyo3 import PsychologicalLine
# from nautilus_trader.core.nautilus_pyo3 import RateOfChange
# from nautilus_trader.core.nautilus_pyo3 import RelativeStrengthIndex
# from nautilus_trader.core.nautilus_pyo3 import RelativeVolatilityIndex
# from nautilus_trader.core.nautilus_pyo3 import SimpleMovingAverage
# from nautilus_trader.core.nautilus_pyo3 import SpreadAnalyzer
# from nautilus_trader.core.nautilus_pyo3 import Stochastics
# from nautilus_trader.core.nautilus_pyo3 import Swings
# from nautilus_trader.core.nautilus_pyo3 import VariableIndexDynamicAverage
# from nautilus_trader.core.nautilus_pyo3 import VerticalHorizontalFilter
# from nautilus_trader.core.nautilus_pyo3 import VolatilityRatio
# from nautilus_trader.core.nautilus_pyo3 import VolumeWeightedAveragePrice
# from nautilus_trader.core.nautilus_pyo3 import WeightedMovingAverage
# from nautilus_trader.core.nautilus_pyo3 import WilderMovingAverage


# __all__ = [
#     "AdaptiveMovingAverage",
#     "ArcherMovingAveragesTrends",
#     "AroonOscillator",
#     "AverageTrueRange",
#     "Bias",
#     "BollingerBands",
#     "BookImbalanceRatio",
#     "CandleDirection",
#     "CandleSize",
#     "CandleBodySize",
#     "CandleWickSize",
#     "ChandeMomentumOscillator",
#     "CommodityChannelIndex",
#     "DonchianChannel",
#     "DoubleExponentialMovingAverage",
#     "EfficiencyRatio",
#     "ExponentialMovingAverage",
#     "FuzzyCandle",
#     "FuzzyCandlesticks",
#     "HullMovingAverage",
#     "KeltnerChannel",
#     "KeltnerPosition",
#     "KlingerVolumeOscillator",
#     "LinearRegression",
#     "MovingAverageConvergenceDivergence",
#     "MovingAverageType",
#     "OnBalanceVolume",
#     "Pressure",
#     "PsychologicalLine",
#     "RateOfChange",
#     "RelativeStrengthIndex",
#     "RelativeVolatilityIndex",
#     "SimpleMovingAverage",
#     "SpreadAnalyzer",
#     "Stochastics",
#     "Swings",
#     "VariableIndexDynamicAverage",
#     "VerticalHorizontalFilter",
#     "VolatilityRatio",
#     "VolumeWeightedAveragePrice",
#     "WeightedMovingAverage",
#     "WilderMovingAverage",
# ]
