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

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use crate::{enums::CurrencyType, types::currency::Currency};

#[must_use]
pub fn currency_map() -> Mutex<HashMap<String, Currency>> {
    Mutex::new(
        [
            // Fiat currencies
            (String::from("AUD"), AUD.clone()),
            (String::from("BRL"), BRL.clone()),
            (String::from("CAD"), CAD.clone()),
            (String::from("CHF"), CHF.clone()),
            (String::from("CNY"), CNY.clone()),
            (String::from("CNH"), CNH.clone()),
            (String::from("CZK"), CZK.clone()),
            (String::from("DKK"), DKK.clone()),
            (String::from("EUR"), EUR.clone()),
            (String::from("GBP"), GBP.clone()),
            (String::from("HKD"), HKD.clone()),
            (String::from("HUF"), HUF.clone()),
            (String::from("ILS"), ILS.clone()),
            (String::from("INR"), INR.clone()),
            (String::from("JPY"), JPY.clone()),
            (String::from("KRW"), KRW.clone()),
            (String::from("MXN"), MXN.clone()),
            (String::from("NOK"), NOK.clone()),
            (String::from("NZD"), NZD.clone()),
            (String::from("PLN"), PLN.clone()),
            (String::from("RUB"), RUB.clone()),
            (String::from("SAR"), SAR.clone()),
            (String::from("SEK"), SEK.clone()),
            (String::from("SGD"), SGD.clone()),
            (String::from("THB"), THB.clone()),
            (String::from("TRY"), TRY.clone()),
            (String::from("USD"), USD.clone()),
            (String::from("XAG"), XAG.clone()),
            (String::from("XAU"), XAU.clone()),
            (String::from("ZAR"), ZAR.clone()),
            // Crypto currencies
            (String::from("1INCH"), ONEINCH.clone()),
            (String::from("AAVE"), AAVE.clone()),
            (String::from("ACA"), ACA.clone()),
            (String::from("ADA"), ADA.clone()),
            (String::from("AVAX"), AVAX.clone()),
            (String::from("BCH"), BCH.clone()),
            (String::from("BTTC"), BTTC.clone()),
            (String::from("BNB"), BNB.clone()),
            (String::from("BRZ"), BRZ.clone()),
            (String::from("BSV"), BSV.clone()),
            (String::from("BTC"), BTC.clone()),
            (String::from("BUSD"), BUSD.clone()),
            (String::from("DASH"), DASH.clone()),
            (String::from("DOGE"), DOGE.clone()),
            (String::from("DOT"), DOT.clone()),
            (String::from("EOS"), EOS.clone()),
            (String::from("ETH"), ETH.clone()),
            (String::from("ETHW"), ETHW.clone()),
            (String::from("JOE"), JOE.clone()),
            (String::from("LINK"), LINK.clone()),
            (String::from("LTC"), LTC.clone()),
            (String::from("LUNA"), LUNA.clone()),
            (String::from("NBT"), NBT.clone()),
            (String::from("SOL"), SOL.clone()),
            (String::from("TRX"), TRX.clone()),
            (String::from("TRYB"), TRYB.clone()),
            (String::from("TUSD"), TUSD.clone()),
            (String::from("VTC"), VTC.clone()),
            (String::from("WSB"), WSB.clone()),
            (String::from("XBT"), XBT.clone()),
            (String::from("XEC"), XEC.clone()),
            (String::from("XLM"), XLM.clone()),
            (String::from("XMR"), XMR.clone()),
            (String::from("XRP"), XRP.clone()),
            (String::from("XTZ"), XTZ.clone()),
            (String::from("USDC"), USDC.clone()),
            (String::from("USDP"), USDP.clone()),
            (String::from("USDT"), USDT.clone()),
            (String::from("ZEC"), ZEC.clone()),
        ]
        .iter()
        .cloned()
        .collect(),
    )
}

lazy_static! {
    // Fiat currencies
    pub static ref AUD: Currency = Currency {
        code: Box::new(Arc::new(String::from("AUD"))),
        precision: 2,
        iso4217: 36,
        name: Box::new(Arc::new(String::from("Australian dollar"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref BRL: Currency = Currency {
        code: Box::new(Arc::new(String::from("BRL"))),
        precision: 2,
        iso4217: 986,
        name: Box::new(Arc::new(String::from("Brazilian real"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CAD: Currency = Currency {
        code: Box::new(Arc::new(String::from("CAD"))),
        precision: 2,
        iso4217: 124,
        name: Box::new(Arc::new(String::from("Canadian dollar"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CHF: Currency = Currency {
        code: Box::new(Arc::new(String::from("CHF"))),
        precision: 2,
        iso4217: 756,
        name: Box::new(Arc::new(String::from("Swiss franc"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CNY: Currency = Currency {
        code: Box::new(Arc::new(String::from("CNY"))),
        precision: 2,
        iso4217: 156,
        name: Box::new(Arc::new(String::from("Chinese yuan"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CNH: Currency = Currency {
        code: Box::new(Arc::new(String::from("CNH"))),
        precision: 2,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Chinese yuan (offshore)"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref CZK: Currency = Currency {
        code: Box::new(Arc::new(String::from("CZK"))),
        precision: 2,
        iso4217: 203,
        name: Box::new(Arc::new(String::from("Czech koruna"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref DKK: Currency = Currency {
        code: Box::new(Arc::new(String::from("DKK"))),
        precision: 2,
        iso4217: 208,
        name: Box::new(Arc::new(String::from("Danish krone"))),
        currency_type: CurrencyType::Fiat,
    };
        pub static ref EUR: Currency = Currency {
        code: Box::new(Arc::new(String::from("EUR"))),
        precision: 2,
        iso4217: 978,
        name: Box::new(Arc::new(String::from("Euro"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref GBP: Currency = Currency {
        code: Box::new(Arc::new(String::from("GBP"))),
        precision: 2,
        iso4217: 826,
        name: Box::new(Arc::new(String::from("British Pound"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref HKD: Currency = Currency {
        code: Box::new(Arc::new(String::from("HKD"))),
        precision: 2,
        iso4217: 344,
        name: Box::new(Arc::new(String::from("Hong Kong dollar"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref HUF: Currency = Currency {
        code: Box::new(Arc::new(String::from("HUF"))),
        precision: 2,
        iso4217: 348,
        name: Box::new(Arc::new(String::from("Hungarian forint"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref ILS: Currency = Currency {
        code: Box::new(Arc::new(String::from("ILS"))),
        precision: 2,
        iso4217: 376,
        name: Box::new(Arc::new(String::from("Israeli new shekel"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref INR: Currency = Currency {
        code: Box::new(Arc::new(String::from("INR"))),
        precision: 2,
        iso4217: 356,
        name: Box::new(Arc::new(String::from("Indian rupee"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref JPY: Currency = Currency {
        code: Box::new(Arc::new(String::from("JPY"))),
        precision: 0,
        iso4217: 392,
        name: Box::new(Arc::new(String::from("Japanese yen"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref KRW: Currency = Currency {
        code: Box::new(Arc::new(String::from("KRW"))),
        precision: 0,
        iso4217: 410,
        name: Box::new(Arc::new(String::from("South Korean won"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref MXN: Currency = Currency {
        code: Box::new(Arc::new(String::from("MXN"))),
        precision: 2,
        iso4217: 484,
        name: Box::new(Arc::new(String::from("Mexican peso"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref NOK: Currency = Currency {
        code: Box::new(Arc::new(String::from("NOK"))),
        precision: 2,
        iso4217: 578,
        name: Box::new(Arc::new(String::from("Norwegian krone"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref NZD: Currency = Currency {
        code: Box::new(Arc::new(String::from("NZD"))),
        precision: 2,
        iso4217: 554,
        name: Box::new(Arc::new(String::from("New Zealand dollar"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref PLN: Currency = Currency {
        code: Box::new(Arc::new(String::from("PLN"))),
        precision: 2,
        iso4217: 985,
        name: Box::new(Arc::new(String::from("Polish złoty"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref RUB: Currency = Currency {
        code: Box::new(Arc::new(String::from("RUB"))),
        precision: 2,
        iso4217: 643,
        name: Box::new(Arc::new(String::from("Russian ruble"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref SAR: Currency = Currency {
        code: Box::new(Arc::new(String::from("SAR"))),
        precision: 2,
        iso4217: 682,
        name: Box::new(Arc::new(String::from("Saudi riyal"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref SEK: Currency = Currency {
        code: Box::new(Arc::new(String::from("SEK"))),
        precision: 2,
        iso4217: 752,
        name: Box::new(Arc::new(String::from("Swedish krona/kronor"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref SGD: Currency = Currency {
        code: Box::new(Arc::new(String::from("SGD"))),
        precision: 2,
        iso4217: 702,
        name: Box::new(Arc::new(String::from("Singapore dollar"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref THB: Currency = Currency {
        code: Box::new(Arc::new(String::from("THB"))),
        precision: 2,
        iso4217: 764,
        name: Box::new(Arc::new(String::from("Thai baht"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref TRY: Currency = Currency {
        code: Box::new(Arc::new(String::from("TRY"))),
        precision: 2,
        iso4217: 949,
        name: Box::new(Arc::new(String::from("Turkish lira"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref USD: Currency = Currency {
        code: Box::new(Arc::new(String::from("USD"))),
        precision: 2,
        iso4217: 840,
        name: Box::new(Arc::new(String::from("United States dollar"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref XAG: Currency = Currency {
        code: Box::new(Arc::new(String::from("XAG"))),
        precision: 0,
        iso4217: 961,
        name: Box::new(Arc::new(String::from("Silver (one troy ounce)"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref XAU: Currency = Currency {
        code: Box::new(Arc::new(String::from("XAU"))),
        precision: 0,
        iso4217: 959,
        name: Box::new(Arc::new(String::from("Gold (one troy ounce)"))),
        currency_type: CurrencyType::Fiat,
    };
    pub static ref ZAR: Currency = Currency {
        code: Box::new(Arc::new(String::from("ZAR"))),
        precision: 2,
        iso4217: 710,
        name: Box::new(Arc::new(String::from("South African rand"))),
        currency_type: CurrencyType::Fiat,
    };
    // Crypto currencies
    pub static ref ONEINCH: Currency = Currency {
        code: Box::new(Arc::new(String::from("1INCH"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("1inch Network"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref AAVE: Currency = Currency {
        code: Box::new(Arc::new(String::from("AAVE"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Aave"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ACA: Currency = Currency {
        code: Box::new(Arc::new(String::from("ACA"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Acala Token"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ADA: Currency = Currency {
        code: Box::new(Arc::new(String::from("ADA"))),
        precision: 6,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Cardano"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref AVAX: Currency = Currency {
        code: Box::new(Arc::new(String::from("AVAX"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Avalanche"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BCH: Currency = Currency {
        code: Box::new(Arc::new(String::from("BCH"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Bitcoin Cash"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BTC: Currency = Currency {
        code: Box::new(Arc::new(String::from("BTC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Bitcoin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BTTC: Currency = Currency {
        code: Box::new(Arc::new(String::from("BTTC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("BitTorrent"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BNB: Currency = Currency {
        code: Box::new(Arc::new(String::from("BNB"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Binance Coin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BRZ: Currency = Currency {
        code: Box::new(Arc::new(String::from("BRZ"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Brazilian Digital Token"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BSV: Currency = Currency {
        code: Box::new(Arc::new(String::from("BSV"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Bitcoin SV"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref BUSD: Currency = Currency {
        code: Box::new(Arc::new(String::from("BUSD"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Binance USD"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref DASH: Currency = Currency {
        code: Box::new(Arc::new(String::from("DASH"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Dash"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref DOGE: Currency = Currency {
        code: Box::new(Arc::new(String::from("DOGE"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Dogecoin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref DOT: Currency = Currency {
        code: Box::new(Arc::new(String::from("DOT"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Polkadot"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref EOS: Currency = Currency {
        code: Box::new(Arc::new(String::from("EOS"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("EOS"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ETH: Currency = Currency {
        code: Box::new(Arc::new(String::from("ETH"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Ether"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ETHW: Currency = Currency {
        code: Box::new(Arc::new(String::from("ETHW"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("EthereumPoW"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref JOE: Currency = Currency {
        code: Box::new(Arc::new(String::from("JOE"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("JOE"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref LINK: Currency = Currency {
        code: Box::new(Arc::new(String::from("LINK"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Chainlink"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref LTC: Currency = Currency {
        code: Box::new(Arc::new(String::from("LTC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Litecoin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref LUNA: Currency = Currency {
        code: Box::new(Arc::new(String::from("LUNA"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Terra"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref NBT: Currency = Currency {
        code: Box::new(Arc::new(String::from("NBT"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("NanoByte Token"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref SOL: Currency = Currency {
        code: Box::new(Arc::new(String::from("SOL"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Solana"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref TRX: Currency = Currency {
        code: Box::new(Arc::new(String::from("TRX"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("TRON"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref TRYB: Currency = Currency {
        code: Box::new(Arc::new(String::from("TRYB"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("BiLira"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref TUSD: Currency = Currency {
        code: Box::new(Arc::new(String::from("TUSD"))),
        precision: 4,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("TrueUSD"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref VTC: Currency = Currency {
        code: Box::new(Arc::new(String::from("VTC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Vertcoin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref WSB: Currency = Currency {
        code: Box::new(Arc::new(String::from("WSB"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("WallStreetBets DApp"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XBT: Currency = Currency {
        code: Box::new(Arc::new(String::from("XBT"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Bitcoin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XEC: Currency = Currency {
        code: Box::new(Arc::new(String::from("XEC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("eCash"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XLM: Currency = Currency {
        code: Box::new(Arc::new(String::from("XLM"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Stellar Lumen"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XMR: Currency = Currency {
        code: Box::new(Arc::new(String::from("XMR"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Monero"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XRP: Currency = Currency {
        code: Box::new(Arc::new(String::from("XRP"))),
        precision: 6,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Ripple"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref XTZ: Currency = Currency {
        code: Box::new(Arc::new(String::from("XTZ"))),
        precision: 6,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Tezos"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref USDC: Currency = Currency {
        code: Box::new(Arc::new(String::from("USDC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("USD Coin"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref USDP: Currency = Currency {
        code: Box::new(Arc::new(String::from("USDP"))),
        precision: 4,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Pax Dollar"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref USDT: Currency = Currency {
        code: Box::new(Arc::new(String::from("USDT"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Tether"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref ZEC: Currency = Currency {
        code: Box::new(Arc::new(String::from("ZEC"))),
        precision: 8,
        iso4217: 0,
        name: Box::new(Arc::new(String::from("Zcash"))),
        currency_type: CurrencyType::Crypto,
    };
    pub static ref CURRENCY_MAP: Mutex<HashMap<String, Currency>> = currency_map();
}
