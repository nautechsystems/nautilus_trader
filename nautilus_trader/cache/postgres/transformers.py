# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.enums import CurrencyType
from nautilus_trader.model.instruments import CryptoFuture
from nautilus_trader.model.instruments import CryptoPerpetual
from nautilus_trader.model.instruments import CurrencyPair
from nautilus_trader.model.instruments import Equity
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.instruments import FuturesSpread
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.instruments import OptionsContract
from nautilus_trader.model.instruments import OptionsSpread
from nautilus_trader.model.objects import Currency


################################################################################
# Currency
################################################################################
def transform_currency_from_pyo3(currency: nautilus_pyo3.Currency) -> Currency:
    return Currency(
        code=currency.code,
        precision=currency.precision,
        iso4217=currency.iso4217,
        name=currency.name,
        currency_type=CurrencyType(currency.currency_type.value),
    )


def transform_currency_to_pyo3(currency: Currency) -> nautilus_pyo3.Currency:
    return nautilus_pyo3.Currency(
        code=currency.code,
        precision=currency.precision,
        iso4217=currency.iso4217,
        name=currency.name,
        currency_type=nautilus_pyo3.CurrencyType.from_str(currency.currency_type.name),
    )


################################################################################
# Instruments
################################################################################


def transform_instrument_to_pyo3(instrument: Instrument):
    if isinstance(instrument, CryptoFuture):
        return nautilus_pyo3.CryptoFuture.from_dict(CryptoFuture.to_dict(instrument))
    elif isinstance(instrument, CryptoPerpetual):
        return nautilus_pyo3.CryptoPerpetual.from_dict(CryptoPerpetual.to_dict(instrument))
    elif isinstance(instrument, CurrencyPair):
        currency_pair_dict = CurrencyPair.to_dict(instrument)
        return nautilus_pyo3.CurrencyPair.from_dict(currency_pair_dict)
    elif isinstance(instrument, Equity):
        return nautilus_pyo3.Equity.from_dict(Equity.to_dict(instrument))
    elif isinstance(instrument, FuturesContract):
        return nautilus_pyo3.FuturesContract.from_dict(FuturesContract.to_dict(instrument))
    elif isinstance(instrument, OptionsContract):
        return nautilus_pyo3.OptionsContract.from_dict(OptionsContract.to_dict(instrument))
    else:
        raise ValueError(f"Unknown instrument type: {instrument}")


def transform_instrument_from_pyo3(instrument_pyo3) -> Instrument | None:
    if instrument_pyo3 is None:
        return None
    if isinstance(instrument_pyo3, nautilus_pyo3.CryptoFuture):
        return CryptoFuture.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.CryptoPerpetual):
        return CryptoPerpetual.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.CurrencyPair):
        return CurrencyPair.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.Equity):
        return Equity.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.FuturesContract):
        return FuturesContract.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.FuturesSpread):
        return FuturesSpread.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.OptionsContract):
        return OptionsContract.from_pyo3(instrument_pyo3)
    elif isinstance(instrument_pyo3, nautilus_pyo3.OptionsSpread):
        return OptionsSpread.from_pyo3(instrument_pyo3)
    else:
        raise ValueError(f"Unknown instrument type: {instrument_pyo3}")
