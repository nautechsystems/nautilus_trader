# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.currency cimport Currency
from nautilus_trader.model.enums_c cimport CurrencyType


# Fiat currencies
AUD = Currency("AUD", precision=2, iso4217=036, name="Australian dollar", currency_type=CurrencyType.FIAT)
BRL = Currency("BRL", precision=2, iso4217=986, name="Brazilian real", currency_type=CurrencyType.FIAT)
CAD = Currency("CAD", precision=2, iso4217=124, name="Canadian dollar", currency_type=CurrencyType.FIAT)
CHF = Currency("CHF", precision=2, iso4217=756, name="Swiss franc", currency_type=CurrencyType.FIAT)
CNY = Currency("CNY", precision=2, iso4217=156, name="Chinese yuan", currency_type=CurrencyType.FIAT)
CNH = Currency("CNH", precision=2, iso4217=0, name="Chinese yuan (offshore)", currency_type=CurrencyType.FIAT)
CZK = Currency("CZK", precision=2, iso4217=203, name="Czech koruna", currency_type=CurrencyType.FIAT)
DKK = Currency("DKK", precision=2, iso4217=208, name="Danish krone", currency_type=CurrencyType.FIAT)
EUR = Currency("EUR", precision=2, iso4217=978, name="Euro", currency_type=CurrencyType.FIAT)
GBP = Currency("GBP", precision=2, iso4217=826, name="British Pound", currency_type=CurrencyType.FIAT)
HKD = Currency("HKD", precision=2, iso4217=344, name="Hong Kong dollar", currency_type=CurrencyType.FIAT)
HUF = Currency("HUF", precision=2, iso4217=348, name="Hungarian forint", currency_type=CurrencyType.FIAT)
ILS = Currency("ILS", precision=2, iso4217=376, name="Israeli new shekel", currency_type=CurrencyType.FIAT)
INR = Currency("INR", precision=2, iso4217=356, name="Indian rupee", currency_type=CurrencyType.FIAT)
JPY = Currency("JPY", precision=0, iso4217=392, name="Japanese yen", currency_type=CurrencyType.FIAT)
KRW = Currency("KRW", precision=0, iso4217=410, name="South Korean won", currency_type=CurrencyType.FIAT)
MXN = Currency("MXN", precision=2, iso4217=484, name="Mexican peso", currency_type=CurrencyType.FIAT)
NOK = Currency("NOK", precision=2, iso4217=578, name="Norwegian krone", currency_type=CurrencyType.FIAT)
NZD = Currency("NZD", precision=2, iso4217=554, name="New Zealand dollar", currency_type=CurrencyType.FIAT)
PLN = Currency("PLN", precision=2, iso4217=985, name="Polish złoty", currency_type=CurrencyType.FIAT)
RUB = Currency("RUB", precision=2, iso4217=643, name="Russian ruble", currency_type=CurrencyType.FIAT)
SAR = Currency("SAR", precision=2, iso4217=682, name="Saudi riyal", currency_type=CurrencyType.FIAT)
SEK = Currency("SEK", precision=2, iso4217=752, name="Swedish krona/kronor", currency_type=CurrencyType.FIAT)
SGD = Currency("SGD", precision=2, iso4217=702, name="Singapore dollar", currency_type=CurrencyType.FIAT)
THB = Currency("THB", precision=2, iso4217=764, name="Thai baht", currency_type=CurrencyType.FIAT)
TRY = Currency("TRY", precision=2, iso4217=949, name="Turkish lira", currency_type=CurrencyType.FIAT)
USD = Currency("USD", precision=2, iso4217=840, name="United States dollar", currency_type=CurrencyType.FIAT)
XAG = Currency("XAG", precision=0, iso4217=961, name="Silver (one troy ounce)", currency_type=CurrencyType.FIAT)
XAU = Currency("XAU", precision=0, iso4217=959, name="Gold (one troy ounce)", currency_type=CurrencyType.FIAT)
ZAR = Currency("ZAR", precision=2, iso4217=710, name="South African rand", currency_type=CurrencyType.FIAT)

# Crypto currencies
ONEINCH = Currency("1INCH", precision=8, iso4217=0, name="1inch Network", currency_type=CurrencyType.CRYPTO)
AAVE = Currency("AAVE", precision=8, iso4217=0, name="Aave", currency_type=CurrencyType.CRYPTO)
ACA = Currency("ACA", precision=8, iso4217=0, name="Acala Token", currency_type=CurrencyType.CRYPTO)
ADA = Currency("ADA", precision=6, iso4217=0, name="Cardano", currency_type=CurrencyType.CRYPTO)
AVAX = Currency("AVAX", precision=8, iso4217=0, name="Avalanche", currency_type=CurrencyType.CRYPTO)
BCH = Currency("BCH", precision=8, iso4217=0, name="Bitcoin Cash", currency_type=CurrencyType.CRYPTO)
BTTC = Currency("BTTC", precision=8, iso4217=0, name="BitTorrent", currency_type=CurrencyType.CRYPTO)
BNB = Currency("BNB", precision=8, iso4217=0, name="Binance Coin", currency_type=CurrencyType.CRYPTO)
BRZ = Currency("BRZ", precision=8, iso4217=0, name="Brazilian Digital Token", currency_type=CurrencyType.CRYPTO)
BSV = Currency("BSV", precision=8, iso4217=0, name="Bitcoin SV", currency_type=CurrencyType.CRYPTO)
BTC = Currency("BTC", precision=8, iso4217=0, name="Bitcoin", currency_type=CurrencyType.CRYPTO)
BUSD = Currency("BUSD", precision=8, iso4217=0, name="Binance USD", currency_type=CurrencyType.CRYPTO)
XBT = Currency("XBT", precision=8, iso4217=0, name="Bitcoin", currency_type=CurrencyType.CRYPTO)
DASH = Currency("DASH", precision=8, iso4217=0, name="Dash", currency_type=CurrencyType.CRYPTO)
DOGE = Currency("DOGE", precision=8, iso4217=0, name="Dogecoin", currency_type=CurrencyType.CRYPTO)
DOT = Currency("DOT", precision=8, iso4217=0, name="Polkadot", currency_type=CurrencyType.CRYPTO)
EOS = Currency("EOS", precision=8, iso4217=0, name="EOS", currency_type=CurrencyType.CRYPTO)
ETH = Currency("ETH", precision=8, iso4217=0, name="Ether", currency_type=CurrencyType.CRYPTO)
ETHW = Currency("ETHW", precision=8, iso4217=0, name="EthereumPoW", currency_type=CurrencyType.CRYPTO)
FTT = Currency("FTT", precision=8, iso4217=0, name="FTT", currency_type=CurrencyType.CRYPTO)
JOE = Currency("JOE", precision=8, iso4217=0, name="JOE", currency_type=CurrencyType.CRYPTO)
LINK = Currency("LINK", precision=8, iso4217=0, name="Chainlink", currency_type=CurrencyType.CRYPTO)
LTC = Currency("LTC", precision=8, iso4217=0, name="Litecoin", currency_type=CurrencyType.CRYPTO)
LUNA = Currency("LUNA", precision=8, iso4217=0, name="Terra", currency_type=CurrencyType.CRYPTO)
NBT = Currency("NBT", precision=8, iso4217=0, name="NanoByte Token", currency_type=CurrencyType.CRYPTO)
SOL = Currency("SOL", precision=8, iso4217=0, name="Solana", currency_type=CurrencyType.CRYPTO)
TRX = Currency("TRX", precision=8, iso4217=0, name="TRON", currency_type=CurrencyType.CRYPTO)
TRYB = Currency("TRYB", precision=8, iso4217=0, name="BiLira", currency_type=CurrencyType.CRYPTO)
VTC = Currency("VTC", precision=8, iso4217=0, name="Vertcoin", currency_type=CurrencyType.CRYPTO)
XLM = Currency("XLM", precision=8, iso4217=0, name="Stellar Lumen", currency_type=CurrencyType.CRYPTO)
XMR = Currency("XMR", precision=8, iso4217=0, name="Monero", currency_type=CurrencyType.CRYPTO)
XRP = Currency("XRP", precision=6, iso4217=0, name="Ripple", currency_type=CurrencyType.CRYPTO)
XTZ = Currency("XTZ", precision=6, iso4217=0, name="Tezos", currency_type=CurrencyType.CRYPTO)
USDC = Currency("USDC", precision=8, iso4217=0, name="USD Coin", currency_type=CurrencyType.CRYPTO)
USDT = Currency("USDT", precision=8, iso4217=0, name="Tether", currency_type=CurrencyType.CRYPTO)
WSB = Currency("WSB", precision=8, iso4217=0, name="WallStreetBets DApp", currency_type=CurrencyType.CRYPTO)
XEC = Currency("XEC", precision=8, iso4217=0, name="eCash", currency_type=CurrencyType.CRYPTO)
ZEC = Currency("ZEC", precision=8, iso4217=0, name="Zcash", currency_type=CurrencyType.CRYPTO)


_CURRENCY_MAP = {
    # Fiat currencies
    "AUD": AUD,
    "BRL": BRL,
    "CAD": CAD,
    "CHF": CHF,
    "CNY": CNY,
    "CNH": CNH,
    "CZK": CZK,
    "DKK": DKK,
    "EUR": EUR,
    "GBP": GBP,
    "HKD": HKD,
    "HUF": HUF,
    "ILS": ILS,
    "INR": INR,
    "JPY": JPY,
    "KRW": KRW,
    "MXN": MXN,
    "NOK": NOK,
    "NZD": NZD,
    "PLN": PLN,
    "RUB": RUB,
    "SAR": SAR,
    "SEK": SEK,
    "SGD": SGD,
    "THB": THB,
    "TRY": TRY,
    "USD": USD,
    "XAG": XAG,
    "XAU": XAU,
    "ZAR": ZAR,
    # Crypto currencies
    "1INCH": ONEINCH,
    "AAVE": AAVE,
    "ACA": ACA,
    "ADA": ADA,
    "AVAX": AVAX,
    "BCH": BCH,
    "BTTC": BTTC,
    "BNB": BNB,
    "BRZ": BRZ,
    "BSV": BSV,
    "BTC": BTC,
    "BUSD": BUSD,
    "XBT": XBT,
    "DASH": DASH,
    "DOGE": DOGE,
    "DOT": DOT,
    "EOS": EOS,
    "ETH": ETH,
    "ETHW": ETHW,
    "FTT": FTT,
    "JOE": JOE,
    "LINK": LINK,
    "LTC": LTC,
    "LUNA": LUNA,
    "NBT": NBT,
    "SOL": SOL,
    "TRX": TRX,
    "TRYB": TRYB,
    "VTC": VTC,
    "XLM": XLM,
    "XMR": XMR,
    "XRP": XRP,
    "XTZ": XTZ,
    "USDC": USDC,
    "USDT": USDT,
    "WSB": WSB,
    "XEC": XEC,
    "ZEC": ZEC,
}
