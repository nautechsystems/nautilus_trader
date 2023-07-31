// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Defines established currency constants and an internal currency map.

use std::{collections::HashMap, sync::Mutex};

use ustr::Ustr;

use crate::{enums::CurrencyType, types::currency::Currency};

#[must_use]
pub fn currency_map() -> Mutex<HashMap<String, Currency>> {
    Mutex::new(
        [
            // Fiat currencies
            (String::from("AUD"), *AUD),
            (String::from("BRL"), *BRL),
            (String::from("CAD"), *CAD),
            (String::from("CHF"), *CHF),
            (String::from("CNY"), *CNY),
            (String::from("CNH"), *CNH),
            (String::from("CZK"), *CZK),
            (String::from("DKK"), *DKK),
            (String::from("EUR"), *EUR),
            (String::from("GBP"), *GBP),
            (String::from("HKD"), *HKD),
            (String::from("HUF"), *HUF),
            (String::from("ILS"), *ILS),
            (String::from("INR"), *INR),
            (String::from("JPY"), *JPY),
            (String::from("KRW"), *KRW),
            (String::from("MXN"), *MXN),
            (String::from("NOK"), *NOK),
            (String::from("NZD"), *NZD),
            (String::from("PLN"), *PLN),
            (String::from("RUB"), *RUB),
            (String::from("SAR"), *SAR),
            (String::from("SEK"), *SEK),
            (String::from("SGD"), *SGD),
            (String::from("THB"), *THB),
            (String::from("TRY"), *TRY),
            (String::from("USD"), *USD),
            (String::from("XAG"), *XAG),
            (String::from("XAU"), *XAU),
            (String::from("ZAR"), *ZAR),
            // Crypto currencies
            (String::from("1INCH"), *ONEINCH),
            (String::from("AAVE"), *AAVE),
            (String::from("ACA"), *ACA),
            (String::from("ADA"), *ADA),
            (String::from("AVAX"), *AVAX),
            (String::from("BCH"), *BCH),
            (String::from("BTTC"), *BTTC),
            (String::from("BNB"), *BNB),
            (String::from("BRZ"), *BRZ),
            (String::from("BSV"), *BSV),
            (String::from("BTC"), *BTC),
            (String::from("BUSD"), *BUSD),
            (String::from("DASH"), *DASH),
            (String::from("DOGE"), *DOGE),
            (String::from("DOT"), *DOT),
            (String::from("EOS"), *EOS),
            (String::from("ETH"), *ETH),
            (String::from("ETHW"), *ETHW),
            (String::from("JOE"), *JOE),
            (String::from("LINK"), *LINK),
            (String::from("LTC"), *LTC),
            (String::from("LUNA"), *LUNA),
            (String::from("NBT"), *NBT),
            (String::from("SOL"), *SOL),
            (String::from("TRX"), *TRX),
            (String::from("TRYB"), *TRYB),
            (String::from("TUSD"), *TUSD),
            (String::from("VTC"), *VTC),
            (String::from("WSB"), *WSB),
            (String::from("XBT"), *XBT),
            (String::from("XEC"), *XEC),
            (String::from("XLM"), *XLM),
            (String::from("XMR"), *XMR),
            (String::from("XRP"), *XRP),
            (String::from("XTZ"), *XTZ),
            (String::from("USDC"), *USDC),
            (String::from("USDP"), *USDP),
            (String::from("USDT"), *USDT),
            (String::from("ZEC"), *ZEC),
        ]
        .iter()
        .cloned()
        .collect(),
    )
}

lazy_static! {
    // Fiat currencies
    pub static ref AUD: Currency = Currency {
        code: Ustr::from("AUD"),
        precision: 2,
        iso4217: 36,
        name: Ustr::from("Australian dollar"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref BRL: Currency = Currency {
        code: Ustr::from("BRL"),
        precision: 2,
        iso4217: 986,
        name: Ustr::from("Brazilian real"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CAD: Currency = Currency {
        code: Ustr::from("CAD"),
        precision: 2,
        iso4217: 124,
        name: Ustr::from("Canadian dollar"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CHF: Currency = Currency {
        code: Ustr::from("CHF"),
        precision: 2,
        iso4217: 756,
        name: Ustr::from("Swiss franc"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CNY: Currency = Currency {
        code: Ustr::from("CNY"),
        precision: 2,
        iso4217: 156,
        name: Ustr::from("Chinese yuan"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CNH: Currency = Currency {
        code: Ustr::from("CNH"),
        precision: 2,
        iso4217: 0,
        name: Ustr::from("Chinese yuan (offshore)"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CZK: Currency = Currency {
        code: Ustr::from("CZK"),
        precision: 2,
        iso4217: 203,
        name: Ustr::from("Czech koruna"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref DKK: Currency = Currency {
        code: Ustr::from("DKK"),
        precision: 2,
        iso4217: 208,
        name: Ustr::from("Danish krone"),
        currency_type: CurrencyType::Fiat,
    };
        pub static ref EUR: Currency = Currency {
        code: Ustr::from("EUR"),
        precision: 2,
        iso4217: 978,
        name: Ustr::from("Euro"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref GBP: Currency = Currency {
        code: Ustr::from("GBP"),
        precision: 2,
        iso4217: 826,
        name: Ustr::from("British Pound"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref HKD: Currency = Currency {
        code: Ustr::from("HKD"),
        precision: 2,
        iso4217: 344,
        name: Ustr::from("Hong Kong dollar"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref HUF: Currency = Currency {
        code: Ustr::from("HUF"),
        precision: 2,
        iso4217: 348,
        name: Ustr::from("Hungarian forint"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref ILS: Currency = Currency {
        code: Ustr::from("ILS"),
        precision: 2,
        iso4217: 376,
        name: Ustr::from("Israeli new shekel"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref INR: Currency = Currency {
        code: Ustr::from("INR"),
        precision: 2,
        iso4217: 356,
        name: Ustr::from("Indian rupee"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref JPY: Currency = Currency {
        code: Ustr::from("JPY"),
        precision: 0,
        iso4217: 392,
        name: Ustr::from("Japanese yen"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref KRW: Currency = Currency {
        code: Ustr::from("KRW"),
        precision: 0,
        iso4217: 410,
        name: Ustr::from("South Korean won"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref MXN: Currency = Currency {
        code: Ustr::from("MXN"),
        precision: 2,
        iso4217: 484,
        name: Ustr::from("Mexican peso"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref NOK: Currency = Currency {
        code: Ustr::from("NOK"),
        precision: 2,
        iso4217: 578,
        name: Ustr::from("Norwegian krone"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref NZD: Currency = Currency {
        code: Ustr::from("NZD"),
        precision: 2,
        iso4217: 554,
        name: Ustr::from("New Zealand dollar"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref PLN: Currency = Currency {
        code: Ustr::from("PLN"),
        precision: 2,
        iso4217: 985,
        name: Ustr::from("Polish złoty"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref RUB: Currency = Currency {
        code: Ustr::from("RUB"),
        precision: 2,
        iso4217: 643,
        name: Ustr::from("Russian ruble"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref SAR: Currency = Currency {
        code: Ustr::from("SAR"),
        precision: 2,
        iso4217: 682,
        name: Ustr::from("Saudi riyal"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref SEK: Currency = Currency {
        code: Ustr::from("SEK"),
        precision: 2,
        iso4217: 752,
        name: Ustr::from("Swedish krona/kronor"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref SGD: Currency = Currency {
        code: Ustr::from("SGD"),
        precision: 2,
        iso4217: 702,
        name: Ustr::from("Singapore dollar"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref THB: Currency = Currency {
        code: Ustr::from("THB"),
        precision: 2,
        iso4217: 764,
        name: Ustr::from("Thai baht"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref TRY: Currency = Currency {
        code: Ustr::from("TRY"),
        precision: 2,
        iso4217: 949,
        name: Ustr::from("Turkish lira"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref USD: Currency = Currency {
        code: Ustr::from("USD"),
        precision: 2,
        iso4217: 840,
        name: Ustr::from("United States dollar"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref XAG: Currency = Currency {
        code: Ustr::from("XAG"),
        precision: 0,
        iso4217: 961,
        name: Ustr::from("Silver (one troy ounce)"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref XAU: Currency = Currency {
        code: Ustr::from("XAU"),
        precision: 0,
        iso4217: 959,
        name: Ustr::from("Gold (one troy ounce)"),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref ZAR: Currency = Currency {
        code: Ustr::from("ZAR"),
        precision: 2,
        iso4217: 710,
        name: Ustr::from("South African rand"),
        currency_type: CurrencyType::Fiat,
    };
    // Crypto currencies
    pub static ref ONEINCH: Currency = Currency {
        code: Ustr::from("1INCH"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("1inch Network"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref AAVE: Currency = Currency {
        code: Ustr::from("AAVE"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Aave"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ACA: Currency = Currency {
        code: Ustr::from("ACA"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Acala Token"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ADA: Currency = Currency {
        code: Ustr::from("ADA"),
        precision: 6,
        iso4217: 0,
        name: Ustr::from("Cardano"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref AVAX: Currency = Currency {
        code: Ustr::from("AVAX"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Avalanche"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BCH: Currency = Currency {
        code: Ustr::from("BCH"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Bitcoin Cash"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BTC: Currency = Currency {
        code: Ustr::from("BTC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Bitcoin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BTTC: Currency = Currency {
        code: Ustr::from("BTTC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("BitTorrent"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BNB: Currency = Currency {
        code: Ustr::from("BNB"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Binance Coin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BRZ: Currency = Currency {
        code: Ustr::from("BRZ"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Brazilian Digital Token"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BSV: Currency = Currency {
        code: Ustr::from("BSV"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Bitcoin SV"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BUSD: Currency = Currency {
        code: Ustr::from("BUSD"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Binance USD"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref DASH: Currency = Currency {
        code: Ustr::from("DASH"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Dash"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref DOGE: Currency = Currency {
        code: Ustr::from("DOGE"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Dogecoin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref DOT: Currency = Currency {
        code: Ustr::from("DOT"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Polkadot"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref EOS: Currency = Currency {
        code: Ustr::from("EOS"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("EOS"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ETH: Currency = Currency {
        code: Ustr::from("ETH"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Ether"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ETHW: Currency = Currency {
        code: Ustr::from("ETHW"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("EthereumPoW"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref JOE: Currency = Currency {
        code: Ustr::from("JOE"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("JOE"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref LINK: Currency = Currency {
        code: Ustr::from("LINK"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Chainlink"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref LTC: Currency = Currency {
        code: Ustr::from("LTC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Litecoin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref LUNA: Currency = Currency {
        code: Ustr::from("LUNA"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Terra"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref NBT: Currency = Currency {
        code: Ustr::from("NBT"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("NanoByte Token"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref SOL: Currency = Currency {
        code: Ustr::from("SOL"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Solana"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref TRX: Currency = Currency {
        code: Ustr::from("TRX"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("TRON"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref TRYB: Currency = Currency {
        code: Ustr::from("TRYB"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("BiLira"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref TUSD: Currency = Currency {
        code: Ustr::from("TUSD"),
        precision: 4,
        iso4217: 0,
        name: Ustr::from("TrueUSD"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref VTC: Currency = Currency {
        code: Ustr::from("VTC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Vertcoin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref WSB: Currency = Currency {
        code: Ustr::from("WSB"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("WallStreetBets DApp"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XBT: Currency = Currency {
        code: Ustr::from("XBT"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Bitcoin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XEC: Currency = Currency {
        code: Ustr::from("XEC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("eCash"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XLM: Currency = Currency {
        code: Ustr::from("XLM"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Stellar Lumen"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XMR: Currency = Currency {
        code: Ustr::from("XMR"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Monero"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XRP: Currency = Currency {
        code: Ustr::from("XRP"),
        precision: 6,
        iso4217: 0,
        name: Ustr::from("Ripple"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XTZ: Currency = Currency {
        code: Ustr::from("XTZ"),
        precision: 6,
        iso4217: 0,
        name: Ustr::from("Tezos"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref USDC: Currency = Currency {
        code: Ustr::from("USDC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("USD Coin"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref USDP: Currency = Currency {
        code: Ustr::from("USDP"),
        precision: 4,
        iso4217: 0,
        name: Ustr::from("Pax Dollar"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref USDT: Currency = Currency {
        code: Ustr::from("USDT"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Tether"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ZEC: Currency = Currency {
        code: Ustr::from("ZEC"),
        precision: 8,
        iso4217: 0,
        name: Ustr::from("Zcash"),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref CURRENCY_MAP: Mutex<HashMap<String, Currency>> = currency_map();
}
