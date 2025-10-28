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

from typing import Final

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.objects import Currency


def register_currency(currency: Currency, overwrite: bool = False) -> None:
    """
    Register a currency in both Cython and PyO3 currency maps.

    This ensures the currency is available to both the Cython-based code
    (nautilus_trader.model.objects.Currency) and PyO3-based code
    (nautilus_trader.core.nautilus_pyo3.Currency).

    Parameters
    ----------
    currency : Currency
        The currency to register.
    overwrite : bool, default False
        If the currency in the internal currency maps should be overwritten.

    """
    Currency.register(currency, overwrite)

    pyo3_currency_type = nautilus_pyo3.CurrencyType.from_str(currency.currency_type.name)

    pyo3_currency = nautilus_pyo3.Currency(
        code=currency.code,
        precision=currency.precision,
        iso4217=currency.iso4217,
        name=currency.name,
        currency_type=pyo3_currency_type,
    )
    nautilus_pyo3.Currency.register(pyo3_currency, overwrite)


# Fiat currencies
AUD: Final[Currency] = Currency.from_internal_map("AUD")
BRL: Final[Currency] = Currency.from_internal_map("BRL")
CAD: Final[Currency] = Currency.from_internal_map("CAD")
CHF: Final[Currency] = Currency.from_internal_map("CHF")
CNY: Final[Currency] = Currency.from_internal_map("CNY")
CNH: Final[Currency] = Currency.from_internal_map("CNH")
CZK: Final[Currency] = Currency.from_internal_map("CZK")
DKK: Final[Currency] = Currency.from_internal_map("DKK")
EUR: Final[Currency] = Currency.from_internal_map("EUR")
GBP: Final[Currency] = Currency.from_internal_map("GBP")
HKD: Final[Currency] = Currency.from_internal_map("HKD")
HUF: Final[Currency] = Currency.from_internal_map("HUF")
ILS: Final[Currency] = Currency.from_internal_map("ILS")
INR: Final[Currency] = Currency.from_internal_map("INR")
JPY: Final[Currency] = Currency.from_internal_map("JPY")
KRW: Final[Currency] = Currency.from_internal_map("KRW")
MXN: Final[Currency] = Currency.from_internal_map("MXN")
NOK: Final[Currency] = Currency.from_internal_map("NOK")
NZD: Final[Currency] = Currency.from_internal_map("NZD")
PLN: Final[Currency] = Currency.from_internal_map("PLN")
RUB: Final[Currency] = Currency.from_internal_map("RUB")
SAR: Final[Currency] = Currency.from_internal_map("SAR")
SEK: Final[Currency] = Currency.from_internal_map("SEK")
SGD: Final[Currency] = Currency.from_internal_map("SGD")
THB: Final[Currency] = Currency.from_internal_map("THB")
TRY: Final[Currency] = Currency.from_internal_map("TRY")
USD: Final[Currency] = Currency.from_internal_map("USD")
XAG: Final[Currency] = Currency.from_internal_map("XAG")
XAU: Final[Currency] = Currency.from_internal_map("XAU")
ZAR: Final[Currency] = Currency.from_internal_map("ZAR")

# Crypto currencies
ONEINCH: Final[Currency] = Currency.from_internal_map("1INCH")
AAVE: Final[Currency] = Currency.from_internal_map("AAVE")
ACA: Final[Currency] = Currency.from_internal_map("ACA")
ADA: Final[Currency] = Currency.from_internal_map("ADA")
ARB: Final[Currency] = Currency.from_internal_map("ARB")
AVAX: Final[Currency] = Currency.from_internal_map("AVAX")
BCH: Final[Currency] = Currency.from_internal_map("BCH")
BIO: Final[Currency] = Currency.from_internal_map("BIO")
BTTC: Final[Currency] = Currency.from_internal_map("BTTC")
BNB: Final[Currency] = Currency.from_internal_map("BNB")
BRZ: Final[Currency] = Currency.from_internal_map("BRZ")
BSV: Final[Currency] = Currency.from_internal_map("BSV")
BTC: Final[Currency] = Currency.from_internal_map("BTC")
BUSD: Final[Currency] = Currency.from_internal_map("BUSD")
XBT: Final[Currency] = Currency.from_internal_map("XBT")
CRV: Final[Currency] = Currency.from_internal_map("CRV")
DASH: Final[Currency] = Currency.from_internal_map("DASH")
DOGE: Final[Currency] = Currency.from_internal_map("DOGE")
DOT: Final[Currency] = Currency.from_internal_map("DOT")
ENA: Final[Currency] = Currency.from_internal_map("ENA")
EOS: Final[Currency] = Currency.from_internal_map("EOS")
ETH: Final[Currency] = Currency.from_internal_map("ETH")
ETHW: Final[Currency] = Currency.from_internal_map("ETHW")
FDUSD: Final[Currency] = Currency.from_internal_map("FDUSD")
EZ: Final[Currency] = Currency.from_internal_map("EZ")
FTT: Final[Currency] = Currency.from_internal_map("FTT")
HYPE: Final[Currency] = Currency.from_internal_map("HYPE")
JOE: Final[Currency] = Currency.from_internal_map("JOE")
LINK: Final[Currency] = Currency.from_internal_map("LINK")
LTC: Final[Currency] = Currency.from_internal_map("LTC")
LUNA: Final[Currency] = Currency.from_internal_map("LUNA")
NBT: Final[Currency] = Currency.from_internal_map("NBT")
PROVE: Final[Currency] = Currency.from_internal_map("PROVE")
SOL: Final[Currency] = Currency.from_internal_map("SOL")
SUI: Final[Currency] = Currency.from_internal_map("SUI")
TRX: Final[Currency] = Currency.from_internal_map("TRX")
TRYB: Final[Currency] = Currency.from_internal_map("TRYB")
TUSD: Final[Currency] = Currency.from_internal_map("TUSD")
UNI: Final[Currency] = Currency.from_internal_map("UNI")
VTC: Final[Currency] = Currency.from_internal_map("VTC")
XLM: Final[Currency] = Currency.from_internal_map("XLM")
XMR: Final[Currency] = Currency.from_internal_map("XMR")
XRP: Final[Currency] = Currency.from_internal_map("XRP")
XTZ: Final[Currency] = Currency.from_internal_map("XTZ")
USDC: Final[Currency] = Currency.from_internal_map("USDC")
USDC_POS: Final[Currency] = Currency.from_internal_map("USDC.e")
USDP: Final[Currency] = Currency.from_internal_map("USDP")
USDT: Final[Currency] = Currency.from_internal_map("USDT")
WSB: Final[Currency] = Currency.from_internal_map("WSB")
XEC: Final[Currency] = Currency.from_internal_map("XEC")
ZEC: Final[Currency] = Currency.from_internal_map("ZEC")
