// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Common `Currency` constants.
//!
//! Precision and metadata references:
//! - ISO 4217 Maintenance Agency dataset (<https://github.com/datasets/currency-codes>):
//!   authoritative alphabetic codes, numeric codes, and minor units for fiat and commodity-backed entries.
//! - Cardano ledger documentation (<https://developers.cardano.org/docs/native-tokens/>):
//!   1 ADA = 1,000,000 lovelace, underpinning the six-decimal crypto precision we retain.
//! - XRPL documentation on drops
//!   (<https://xrpl.org/docs/references/protocol/data-types/currency-formats>):
//!   1 XRP = 1,000,000 drops, confirming the six-decimal allowance for XRP.
//! - Tezos protocol reference (<https://tezos.gitlab.io/active/numismatics.html>):
//!   1 tez = 1,000,000 mutez, informing the six-decimal precision for XTZ.
//! - Stablecoin contract metadata for USDC, USDP, BRZ, and USDG
//!   (e.g. <https://etherscan.io/token/0xa0b86991c6218b36c1d19d4a2e9eb0ce3606eb48#readContract>,
//!   <https://etherscan.io/token/0x8e870d67f660d95d5be530380d0ec0bd388289e1#readContract>,
//!   <https://etherscan.io/token/0x01d33fd36ec67c6ada32cf36b31e88ee190b1839#readContract>,
//!   <https://github.com/paxosglobal/usdg-contract/blob/5afb581e076f69ae46eb2e360f4dc63a71514a78/contracts/USDG.sol>):
//!   each exposes 6-18 on-chain decimals; we clamp to an 8-decimal internal default.

use std::{
    collections::HashMap,
    sync::{LazyLock, OnceLock},
};

use parking_lot::Mutex;
use ustr::Ustr;

use crate::{enums::CurrencyType, types::Currency};

/// Declares the built-in [`Currency`] constants.
///
/// Each row generates the accessor, its backing `OnceLock`, and the [`CURRENCY_MAP`] registration,
/// so a currency cannot be declared without also being registered.
macro_rules! currency_constants {
    ($(
        $accessor:ident => $code:literal, $precision:literal, $iso4217:literal, $name:literal,
        $currency_type:ident;
    )+) => {
        impl Currency {
            $(
                #[doc = concat!("Returns the ", $name, " (`", $code, "`) currency.")]
                #[allow(non_snake_case)]
                #[must_use]
                pub fn $accessor() -> Self {
                    static LOCK: OnceLock<Currency> = OnceLock::new();
                    *LOCK.get_or_init(|| Self {
                        code: Ustr::from($code),
                        precision: $precision,
                        iso4217: $iso4217,
                        name: Ustr::from($name),
                        currency_type: CurrencyType::$currency_type,
                    })
                }
            )+
        }

        /// A map of built-in `Currency` constants.
        pub static CURRENCY_MAP: LazyLock<Mutex<HashMap<String, Currency>>> = LazyLock::new(|| {
            let mut map = HashMap::new();
            $(map.insert(Currency::$accessor().code.to_string(), Currency::$accessor());)+
            Mutex::new(map)
        });

        #[cfg(test)]
        fn all_currency_constants() -> Vec<Currency> {
            vec![$(Currency::$accessor()),+]
        }
    };
}

currency_constants! {
    // Fiat currencies
    AUD => "AUD", 2, 36, "Australian dollar", Fiat;
    BRL => "BRL", 2, 986, "Brazilian real", Fiat;
    CAD => "CAD", 2, 124, "Canadian dollar", Fiat;
    CHF => "CHF", 2, 756, "Swiss franc", Fiat;
    CNY => "CNY", 2, 156, "Chinese yuan", Fiat;
    CNH => "CNH", 2, 0, "Chinese yuan (offshore)", Fiat;
    CZK => "CZK", 2, 203, "Czech koruna", Fiat;
    DKK => "DKK", 2, 208, "Danish krone", Fiat;
    EUR => "EUR", 2, 978, "Euro", Fiat;
    GBP => "GBP", 2, 826, "British Pound", Fiat;
    HKD => "HKD", 2, 344, "Hong Kong dollar", Fiat;
    HUF => "HUF", 2, 348, "Hungarian forint", Fiat;
    ILS => "ILS", 2, 376, "Israeli new shekel", Fiat;
    INR => "INR", 2, 356, "Indian rupee", Fiat;
    JPY => "JPY", 0, 392, "Japanese yen", Fiat;
    KRW => "KRW", 0, 410, "South Korean won", Fiat;
    MXN => "MXN", 2, 484, "Mexican peso", Fiat;
    NOK => "NOK", 2, 578, "Norwegian krone", Fiat;
    NZD => "NZD", 2, 554, "New Zealand dollar", Fiat;
    PLN => "PLN", 2, 985, "Polish złoty", Fiat;
    RUB => "RUB", 2, 643, "Russian ruble", Fiat;
    SAR => "SAR", 2, 682, "Saudi riyal", Fiat;
    SEK => "SEK", 2, 752, "Swedish krona", Fiat;
    SGD => "SGD", 2, 702, "Singapore dollar", Fiat;
    THB => "THB", 2, 764, "Thai baht", Fiat;
    TRY => "TRY", 2, 949, "Turkish lira", Fiat;
    TWD => "TWD", 2, 901, "New Taiwan dollar", Fiat;
    USD => "USD", 2, 840, "United States dollar", Fiat;
    ZAR => "ZAR", 2, 710, "South African rand", Fiat;

    // Commodity backed currencies
    XAG => "XAG", 2, 961, "Silver (one troy ounce)", CommodityBacked;
    XAU => "XAU", 2, 959, "Gold (one troy ounce)", CommodityBacked;
    XPT => "XPT", 2, 962, "Platinum (one troy ounce)", CommodityBacked;

    // Crypto currencies
    ONEINCH => "1INCH", 8, 0, "1inch Network", Crypto;
    AAVE => "AAVE", 8, 0, "Aave", Crypto;
    ACA => "ACA", 8, 0, "Acala Token", Crypto;
    ADA => "ADA", 6, 0, "Cardano", Crypto;
    APT => "APT", 8, 0, "Aptos", Crypto;
    ARB => "ARB", 8, 0, "Arbitrum", Crypto;
    AVAX => "AVAX", 8, 0, "Avalanche", Crypto;
    BCH => "BCH", 8, 0, "Bitcoin Cash", Crypto;
    BIO => "BIO", 8, 0, "BioPassport", Crypto;
    BTC => "BTC", 8, 0, "Bitcoin", Crypto;
    BTTC => "BTTC", 8, 0, "BitTorrent", Crypto;
    BNB => "BNB", 8, 0, "Binance Coin", Crypto;
    BRZ => "BRZ", 8, 0, "Brazilian Digital Token", Crypto;
    BSV => "BSV", 8, 0, "Bitcoin SV", Crypto;
    BUSD => "BUSD", 8, 0, "Binance USD", Crypto;
    CAKE => "CAKE", 8, 0, "PancakeSwap", Crypto;
    CRV => "CRV", 8, 0, "Curve DAO Token", Crypto;
    DASH => "DASH", 8, 0, "Dash", Crypto;
    DOT => "DOT", 8, 0, "Polkadot", Crypto;
    DOGE => "DOGE", 8, 0, "Dogecoin", Crypto;
    ENA => "ENA", 8, 0, "Ethena", Crypto;
    EOS => "EOS", 8, 0, "EOS", Crypto;
    ETH => "ETH", 8, 0, "Ethereum", Crypto;
    ETHW => "ETHW", 8, 0, "EthereumPoW", Crypto;
    FDUSD => "FDUSD", 8, 0, "First Digital USD", Crypto;
    GWEI => "GWEI", 8, 0, "Gwei", Crypto;
    HYPE => "HYPE", 8, 0, "Hyperliquid", Crypto;
    JOE => "JOE", 8, 0, "JOE", Crypto;
    LINK => "LINK", 8, 0, "Chainlink", Crypto;
    LTC => "LTC", 8, 0, "Litecoin", Crypto;
    LUNA => "LUNA", 8, 0, "Terra", Crypto;
    MAMUSD => "MAMUSD", 8, 0, "MAMUSD", Crypto;
    NBT => "NBT", 8, 0, "NanoByte Token", Crypto;
    POL => "POL", 8, 0, "Polygon", Crypto;
    PROVE => "PROVE", 8, 0, "Prove AI", Crypto;
    RLUSD => "RLUSD", 8, 0, "Ripple USD", Crypto;
    SOL => "SOL", 8, 0, "Solana", Crypto;
    SHIB => "SHIB", 8, 0, "Shiba Inu", Crypto;
    SUI => "SUI", 8, 0, "Sui", Crypto;
    TON => "TON", 8, 0, "Toncoin", Crypto;
    TRX => "TRX", 8, 0, "TRON", Crypto;
    TRYB => "TRYB", 8, 0, "BiLira", Crypto;
    TUSD => "TUSD", 8, 0, "TrueUSD", Crypto;
    UNI => "UNI", 8, 0, "Uniswap", Crypto;
    VTC => "VTC", 8, 0, "Vertcoin", Crypto;
    WBTC => "WBTC", 8, 0, "Wrapped Bitcoin", Crypto;
    WSB => "WSB", 8, 0, "WallStreetBets DApp", Crypto;
    XBT => "XBT", 8, 0, "Bitcoin", Crypto;
    XEC => "XEC", 8, 0, "eCash", Crypto;
    XLM => "XLM", 8, 0, "Stellar Lumen", Crypto;
    XMR => "XMR", 8, 0, "Monero", Crypto;
    USDT => "USDT", 8, 0, "Tether", Crypto;
    XRP => "XRP", 6, 0, "XRP", Crypto;
    XTZ => "XTZ", 6, 0, "Tezos", Crypto;
    USDC => "USDC", 8, 0, "USD Coin", Crypto;
    USDC_POS => "USDC.e", 6, 0, "USD Coin (PoS)", Crypto;
    USDG => "USDG", 8, 0, "Global Dollar", Crypto;
    USDP => "USDP", 8, 0, "Pax Dollar", Crypto;
    pUSD => "pUSD", 6, 0, "Polymarket USD", Crypto;
    ZEC => "ZEC", 8, 0, "Zcash", Crypto;
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;

    use super::*;
    use crate::types::fixed::FIXED_PRECISION;

    #[rstest]
    fn test_every_constant_is_registered_and_round_trips_by_code() {
        for currency in all_currency_constants() {
            let code = currency.code.as_str();

            // `Currency::PartialEq` compares the code alone, and `try_from_str` and `From` are
            // separate lookup bodies, so both need asserting field by field.
            let try_from = Currency::try_from_str(code).expect("constant must be registered");
            let from_str = Currency::from(code);

            for (label, found) in [("try_from_str", try_from), ("from", from_str)] {
                assert_eq!(found.code, currency.code, "{code} via {label}");
                assert_eq!(found.precision, currency.precision, "{code} via {label}");
                assert_eq!(found.iso4217, currency.iso4217, "{code} via {label}");
                assert_eq!(found.name, currency.name, "{code} via {label}");
                assert_eq!(
                    found.currency_type, currency.currency_type,
                    "{code} via {label}",
                );
            }
        }
    }

    /// Every registered currency as `CODE|precision|iso4217|name|TYPE`.
    ///
    /// Captured from the hand-written definitions that `currency_constants!` replaced. The
    /// accessors, the registry and the other tests all derive from the macro table, so this is the
    /// only independent statement of each currency's metadata: editing a table row without
    /// editing this list fails, which is what pins monetary precision.
    const EXPECTED_CURRENCIES: &[&str] = &[
        "1INCH|8|0|1inch Network|Crypto",
        "AAVE|8|0|Aave|Crypto",
        "ACA|8|0|Acala Token|Crypto",
        "ADA|6|0|Cardano|Crypto",
        "APT|8|0|Aptos|Crypto",
        "ARB|8|0|Arbitrum|Crypto",
        "AUD|2|36|Australian dollar|Fiat",
        "AVAX|8|0|Avalanche|Crypto",
        "BCH|8|0|Bitcoin Cash|Crypto",
        "BIO|8|0|BioPassport|Crypto",
        "BNB|8|0|Binance Coin|Crypto",
        "BRL|2|986|Brazilian real|Fiat",
        "BRZ|8|0|Brazilian Digital Token|Crypto",
        "BSV|8|0|Bitcoin SV|Crypto",
        "BTC|8|0|Bitcoin|Crypto",
        "BTTC|8|0|BitTorrent|Crypto",
        "BUSD|8|0|Binance USD|Crypto",
        "CAD|2|124|Canadian dollar|Fiat",
        "CAKE|8|0|PancakeSwap|Crypto",
        "CHF|2|756|Swiss franc|Fiat",
        "CNH|2|0|Chinese yuan (offshore)|Fiat",
        "CNY|2|156|Chinese yuan|Fiat",
        "CRV|8|0|Curve DAO Token|Crypto",
        "CZK|2|203|Czech koruna|Fiat",
        "DASH|8|0|Dash|Crypto",
        "DKK|2|208|Danish krone|Fiat",
        "DOGE|8|0|Dogecoin|Crypto",
        "DOT|8|0|Polkadot|Crypto",
        "ENA|8|0|Ethena|Crypto",
        "EOS|8|0|EOS|Crypto",
        "ETHW|8|0|EthereumPoW|Crypto",
        "ETH|8|0|Ethereum|Crypto",
        "EUR|2|978|Euro|Fiat",
        "FDUSD|8|0|First Digital USD|Crypto",
        "GBP|2|826|British Pound|Fiat",
        "GWEI|8|0|Gwei|Crypto",
        "HKD|2|344|Hong Kong dollar|Fiat",
        "HUF|2|348|Hungarian forint|Fiat",
        "HYPE|8|0|Hyperliquid|Crypto",
        "ILS|2|376|Israeli new shekel|Fiat",
        "INR|2|356|Indian rupee|Fiat",
        "JOE|8|0|JOE|Crypto",
        "JPY|0|392|Japanese yen|Fiat",
        "KRW|0|410|South Korean won|Fiat",
        "LINK|8|0|Chainlink|Crypto",
        "LTC|8|0|Litecoin|Crypto",
        "LUNA|8|0|Terra|Crypto",
        "MAMUSD|8|0|MAMUSD|Crypto",
        "MXN|2|484|Mexican peso|Fiat",
        "NBT|8|0|NanoByte Token|Crypto",
        "NOK|2|578|Norwegian krone|Fiat",
        "NZD|2|554|New Zealand dollar|Fiat",
        "PLN|2|985|Polish złoty|Fiat",
        "POL|8|0|Polygon|Crypto",
        "PROVE|8|0|Prove AI|Crypto",
        "RLUSD|8|0|Ripple USD|Crypto",
        "RUB|2|643|Russian ruble|Fiat",
        "SAR|2|682|Saudi riyal|Fiat",
        "SEK|2|752|Swedish krona|Fiat",
        "SGD|2|702|Singapore dollar|Fiat",
        "SHIB|8|0|Shiba Inu|Crypto",
        "SOL|8|0|Solana|Crypto",
        "SUI|8|0|Sui|Crypto",
        "THB|2|764|Thai baht|Fiat",
        "TON|8|0|Toncoin|Crypto",
        "TRX|8|0|TRON|Crypto",
        "TRYB|8|0|BiLira|Crypto",
        "TRY|2|949|Turkish lira|Fiat",
        "TUSD|8|0|TrueUSD|Crypto",
        "TWD|2|901|New Taiwan dollar|Fiat",
        "UNI|8|0|Uniswap|Crypto",
        "USDC.e|6|0|USD Coin (PoS)|Crypto",
        "USDC|8|0|USD Coin|Crypto",
        "USDG|8|0|Global Dollar|Crypto",
        "USDP|8|0|Pax Dollar|Crypto",
        "USDT|8|0|Tether|Crypto",
        "USD|2|840|United States dollar|Fiat",
        "VTC|8|0|Vertcoin|Crypto",
        "WBTC|8|0|Wrapped Bitcoin|Crypto",
        "WSB|8|0|WallStreetBets DApp|Crypto",
        "XAG|2|961|Silver (one troy ounce)|CommodityBacked",
        "XAU|2|959|Gold (one troy ounce)|CommodityBacked",
        "XBT|8|0|Bitcoin|Crypto",
        "XEC|8|0|eCash|Crypto",
        "XLM|8|0|Stellar Lumen|Crypto",
        "XMR|8|0|Monero|Crypto",
        "XPT|2|962|Platinum (one troy ounce)|CommodityBacked",
        "XRP|6|0|XRP|Crypto",
        "XTZ|6|0|Tezos|Crypto",
        "ZAR|2|710|South African rand|Fiat",
        "ZEC|8|0|Zcash|Crypto",
        "pUSD|6|0|Polymarket USD|Crypto",
    ];

    #[rstest]
    fn test_registered_currency_metadata_matches_the_pinned_list() {
        let mut actual: Vec<String> = all_currency_constants()
            .iter()
            .map(|c| {
                format!(
                    "{}|{}|{}|{}|{:?}",
                    c.code, c.precision, c.iso4217, c.name, c.currency_type
                )
            })
            .collect();
        actual.sort();

        assert_eq!(
            actual.len(),
            EXPECTED_CURRENCIES.len(),
            "currency count changed; update `EXPECTED_CURRENCIES`",
        );

        for (got, expected) in actual.iter().zip(EXPECTED_CURRENCIES) {
            assert_eq!(got, expected);
        }
    }

    #[rstest]
    fn test_currency_codes_are_unique() {
        // Two rows sharing a code would collapse into a single registry entry.
        let constants = all_currency_constants();
        let codes: HashSet<Ustr> = constants.iter().map(|c| c.code).collect();

        assert_eq!(
            codes.len(),
            constants.len(),
            "currency codes must be unique"
        );
    }

    #[rstest]
    fn test_registered_currencies_satisfy_value_invariants() {
        for currency in all_currency_constants() {
            let code = currency.code;

            assert!(!currency.name.is_empty(), "{code} must carry a name");
            assert!(
                currency.precision <= FIXED_PRECISION,
                "{code} precision {} exceeds `FIXED_PRECISION` {FIXED_PRECISION}",
                currency.precision,
            );

            if currency.currency_type == CurrencyType::Crypto {
                assert_eq!(
                    currency.iso4217, 0,
                    "{code} is crypto and must not carry an ISO 4217 code",
                );
            }
        }
    }

    #[rstest]
    fn test_unknown_codes_are_not_registered() {
        assert_eq!(Currency::try_from_str("NOT_A_CURRENCY"), None);
        assert_eq!(Currency::try_from_str(""), None);
        assert_eq!(
            Currency::try_from_str("btc"),
            None,
            "lookup is case sensitive"
        );
    }
}
