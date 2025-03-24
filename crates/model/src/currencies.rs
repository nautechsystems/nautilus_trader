// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{
    collections::HashMap,
    sync::{LazyLock, Mutex, OnceLock},
};

use ustr::Ustr;

use crate::{enums::CurrencyType, types::Currency};

///////////////////////////////////////////////////////////////////////////////
// Fiat currencies
///////////////////////////////////////////////////////////////////////////////
static AUD_LOCK: OnceLock<Currency> = OnceLock::new();
static BRL_LOCK: OnceLock<Currency> = OnceLock::new();
static CAD_LOCK: OnceLock<Currency> = OnceLock::new();
static CHF_LOCK: OnceLock<Currency> = OnceLock::new();
static CNY_LOCK: OnceLock<Currency> = OnceLock::new();
static CNH_LOCK: OnceLock<Currency> = OnceLock::new();
static CZK_LOCK: OnceLock<Currency> = OnceLock::new();
static DKK_LOCK: OnceLock<Currency> = OnceLock::new();
static EUR_LOCK: OnceLock<Currency> = OnceLock::new();
static GBP_LOCK: OnceLock<Currency> = OnceLock::new();
static HKD_LOCK: OnceLock<Currency> = OnceLock::new();
static HUF_LOCK: OnceLock<Currency> = OnceLock::new();
static ILS_LOCK: OnceLock<Currency> = OnceLock::new();
static INR_LOCK: OnceLock<Currency> = OnceLock::new();
static JPY_LOCK: OnceLock<Currency> = OnceLock::new();
static KRW_LOCK: OnceLock<Currency> = OnceLock::new();
static MXN_LOCK: OnceLock<Currency> = OnceLock::new();
static NOK_LOCK: OnceLock<Currency> = OnceLock::new();
static NZD_LOCK: OnceLock<Currency> = OnceLock::new();
static PLN_LOCK: OnceLock<Currency> = OnceLock::new();
static RUB_LOCK: OnceLock<Currency> = OnceLock::new();
static SAR_LOCK: OnceLock<Currency> = OnceLock::new();
static SEK_LOCK: OnceLock<Currency> = OnceLock::new();
static SGD_LOCK: OnceLock<Currency> = OnceLock::new();
static THB_LOCK: OnceLock<Currency> = OnceLock::new();
static TRY_LOCK: OnceLock<Currency> = OnceLock::new();
static TWD_LOCK: OnceLock<Currency> = OnceLock::new();
static USD_LOCK: OnceLock<Currency> = OnceLock::new();
static ZAR_LOCK: OnceLock<Currency> = OnceLock::new();

///////////////////////////////////////////////////////////////////////////////
// Commodity backed currencies
///////////////////////////////////////////////////////////////////////////////
static XAG_LOCK: OnceLock<Currency> = OnceLock::new();
static XAU_LOCK: OnceLock<Currency> = OnceLock::new();
static XPT_LOCK: OnceLock<Currency> = OnceLock::new();

///////////////////////////////////////////////////////////////////////////////
// Crypto currencies
///////////////////////////////////////////////////////////////////////////////
static ONEINCH_LOCK: OnceLock<Currency> = OnceLock::new();
static AAVE_LOCK: OnceLock<Currency> = OnceLock::new();
static ACA_LOCK: OnceLock<Currency> = OnceLock::new();
static ADA_LOCK: OnceLock<Currency> = OnceLock::new();
static AVAX_LOCK: OnceLock<Currency> = OnceLock::new();
static BCH_LOCK: OnceLock<Currency> = OnceLock::new();
static BTC_LOCK: OnceLock<Currency> = OnceLock::new();
static BTTC_LOCK: OnceLock<Currency> = OnceLock::new();
static BNB_LOCK: OnceLock<Currency> = OnceLock::new();
static BRZ_LOCK: OnceLock<Currency> = OnceLock::new();
static BSV_LOCK: OnceLock<Currency> = OnceLock::new();
static BUSD_LOCK: OnceLock<Currency> = OnceLock::new();
static CAKE_LOCK: OnceLock<Currency> = OnceLock::new();
static DASH_LOCK: OnceLock<Currency> = OnceLock::new();
static DOGE_LOCK: OnceLock<Currency> = OnceLock::new();
static DOT_LOCK: OnceLock<Currency> = OnceLock::new();
static EOS_LOCK: OnceLock<Currency> = OnceLock::new();
static ETH_LOCK: OnceLock<Currency> = OnceLock::new();
static ETHW_LOCK: OnceLock<Currency> = OnceLock::new();
static FDUSD_LOCK: OnceLock<Currency> = OnceLock::new();
static JOE_LOCK: OnceLock<Currency> = OnceLock::new();
static LINK_LOCK: OnceLock<Currency> = OnceLock::new();
static LTC_LOCK: OnceLock<Currency> = OnceLock::new();
static LUNA_LOCK: OnceLock<Currency> = OnceLock::new();
static NBT_LOCK: OnceLock<Currency> = OnceLock::new();
static SOL_LOCK: OnceLock<Currency> = OnceLock::new();
static TRX_LOCK: OnceLock<Currency> = OnceLock::new();
static TRYB_LOCK: OnceLock<Currency> = OnceLock::new();
static TUSD_LOCK: OnceLock<Currency> = OnceLock::new();
static SHIB_LOCK: OnceLock<Currency> = OnceLock::new();
static VTC_LOCK: OnceLock<Currency> = OnceLock::new();
static WSB_LOCK: OnceLock<Currency> = OnceLock::new();
static XBT_LOCK: OnceLock<Currency> = OnceLock::new();
static XEC_LOCK: OnceLock<Currency> = OnceLock::new();
static XLM_LOCK: OnceLock<Currency> = OnceLock::new();
static XMR_LOCK: OnceLock<Currency> = OnceLock::new();
static XRP_LOCK: OnceLock<Currency> = OnceLock::new();
static XTZ_LOCK: OnceLock<Currency> = OnceLock::new();
static USDC_LOCK: OnceLock<Currency> = OnceLock::new();
static USDC_POS_LOCK: OnceLock<Currency> = OnceLock::new();
static USDP_LOCK: OnceLock<Currency> = OnceLock::new();
static USDT_LOCK: OnceLock<Currency> = OnceLock::new();
static ZEC_LOCK: OnceLock<Currency> = OnceLock::new();

impl Currency {
    ///////////////////////////////////////////////////////////////////////////
    // Fiat currencies
    ///////////////////////////////////////////////////////////////////////////
    #[allow(non_snake_case)]
    #[must_use]
    pub fn AUD() -> Self {
        *AUD_LOCK.get_or_init(|| Self {
            code: Ustr::from("AUD"),
            precision: 2,
            iso4217: 36,
            name: Ustr::from("Australian dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }
    #[allow(non_snake_case)]
    #[must_use]
    pub fn BRL() -> Self {
        *BRL_LOCK.get_or_init(|| Self {
            code: Ustr::from("BRL"),
            precision: 2,
            iso4217: 986,
            name: Ustr::from("Brazilian real"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn CAD() -> Self {
        *CAD_LOCK.get_or_init(|| Self {
            code: Ustr::from("CAD"),
            precision: 2,
            iso4217: 124,
            name: Ustr::from("Canadian dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn CHF() -> Self {
        *CHF_LOCK.get_or_init(|| Self {
            code: Ustr::from("CHF"),
            precision: 2,
            iso4217: 756,
            name: Ustr::from("Swiss franc"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn CNY() -> Self {
        *CNY_LOCK.get_or_init(|| Self {
            code: Ustr::from("CNY"),
            precision: 2,
            iso4217: 156,
            name: Ustr::from("Chinese yuan"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn CNH() -> Self {
        *CNH_LOCK.get_or_init(|| Self {
            code: Ustr::from("CNH"),
            precision: 2,
            iso4217: 0,
            name: Ustr::from("Chinese yuan (offshore)"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn CZK() -> Self {
        *CZK_LOCK.get_or_init(|| Self {
            code: Ustr::from("CZK"),
            precision: 2,
            iso4217: 203,
            name: Ustr::from("Czech koruna"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn DKK() -> Self {
        *DKK_LOCK.get_or_init(|| Self {
            code: Ustr::from("DKK"),
            precision: 2,
            iso4217: 208,
            name: Ustr::from("Danish krone"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn EUR() -> Self {
        *EUR_LOCK.get_or_init(|| Self {
            code: Ustr::from("EUR"),
            precision: 2,
            iso4217: 978,
            name: Ustr::from("Euro"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn GBP() -> Self {
        *GBP_LOCK.get_or_init(|| Self {
            code: Ustr::from("GBP"),
            precision: 2,
            iso4217: 826,
            name: Ustr::from("British Pound"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn HKD() -> Self {
        *HKD_LOCK.get_or_init(|| Self {
            code: Ustr::from("HKD"),
            precision: 2,
            iso4217: 344,
            name: Ustr::from("Hong Kong dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn HUF() -> Self {
        *HUF_LOCK.get_or_init(|| Self {
            code: Ustr::from("HUF"),
            precision: 2,
            iso4217: 348,
            name: Ustr::from("Hungarian forint"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn ILS() -> Self {
        *ILS_LOCK.get_or_init(|| Self {
            code: Ustr::from("ILS"),
            precision: 2,
            iso4217: 376,
            name: Ustr::from("Israeli new shekel"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn INR() -> Self {
        *INR_LOCK.get_or_init(|| Self {
            code: Ustr::from("INR"),
            precision: 2,
            iso4217: 356,
            name: Ustr::from("Indian rupee"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn JPY() -> Self {
        *JPY_LOCK.get_or_init(|| Self {
            code: Ustr::from("JPY"),
            precision: 0,
            iso4217: 392,
            name: Ustr::from("Japanese yen"),
            currency_type: CurrencyType::Fiat,
        })
    }
    #[allow(non_snake_case)]
    #[must_use]
    pub fn KRW() -> Self {
        *KRW_LOCK.get_or_init(|| Self {
            code: Ustr::from("KRW"),
            precision: 0,
            iso4217: 410,
            name: Ustr::from("South Korean won"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn MXN() -> Self {
        *MXN_LOCK.get_or_init(|| Self {
            code: Ustr::from("MXN"),
            precision: 2,
            iso4217: 484,
            name: Ustr::from("Mexican peso"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn NOK() -> Self {
        *NOK_LOCK.get_or_init(|| Self {
            code: Ustr::from("NOK"),
            precision: 2,
            iso4217: 578,
            name: Ustr::from("Norwegian krone"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn NZD() -> Self {
        *NZD_LOCK.get_or_init(|| Self {
            code: Ustr::from("NZD"),
            precision: 2,
            iso4217: 554,
            name: Ustr::from("New Zealand dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn PLN() -> Self {
        *PLN_LOCK.get_or_init(|| Self {
            code: Ustr::from("PLN"),
            precision: 2,
            iso4217: 985,
            name: Ustr::from("Polish złoty"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn RUB() -> Self {
        *RUB_LOCK.get_or_init(|| Self {
            code: Ustr::from("RUB"),
            precision: 2,
            iso4217: 643,
            name: Ustr::from("Russian ruble"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn SAR() -> Self {
        *SAR_LOCK.get_or_init(|| Self {
            code: Ustr::from("SAR"),
            precision: 2,
            iso4217: 682,
            name: Ustr::from("Saudi riyal"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn SEK() -> Self {
        *SEK_LOCK.get_or_init(|| Self {
            code: Ustr::from("SEK"),
            precision: 2,
            iso4217: 752,
            name: Ustr::from("Swedish krona"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn SGD() -> Self {
        *SGD_LOCK.get_or_init(|| Self {
            code: Ustr::from("SGD"),
            precision: 2,
            iso4217: 702,
            name: Ustr::from("Singapore dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn THB() -> Self {
        *THB_LOCK.get_or_init(|| Self {
            code: Ustr::from("THB"),
            precision: 2,
            iso4217: 764,
            name: Ustr::from("Thai baht"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn TRY() -> Self {
        *TRY_LOCK.get_or_init(|| Self {
            code: Ustr::from("TRY"),
            precision: 2,
            iso4217: 949,
            name: Ustr::from("Turkish lira"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn TWD() -> Self {
        *TWD_LOCK.get_or_init(|| Self {
            code: Ustr::from("TWD"),
            precision: 2,
            iso4217: 901,
            name: Ustr::from("New Taiwan dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn USD() -> Self {
        *USD_LOCK.get_or_init(|| Self {
            code: Ustr::from("USD"),
            precision: 2,
            iso4217: 840,
            name: Ustr::from("United States dollar"),
            currency_type: CurrencyType::Fiat,
        })
    }
    #[allow(non_snake_case)]
    #[must_use]
    pub fn ZAR() -> Self {
        *ZAR_LOCK.get_or_init(|| Self {
            code: Ustr::from("ZAR"),
            precision: 2,
            iso4217: 710,
            name: Ustr::from("South African rand"),
            currency_type: CurrencyType::Fiat,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XAG() -> Self {
        *XAG_LOCK.get_or_init(|| Self {
            code: Ustr::from("XAG"),
            precision: 2,
            iso4217: 961,
            name: Ustr::from("Silver (one troy ounce)"),
            currency_type: CurrencyType::CommodityBacked,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XAU() -> Self {
        *XAU_LOCK.get_or_init(|| Self {
            code: Ustr::from("XAU"),
            precision: 2,
            iso4217: 959,
            name: Ustr::from("Gold (one troy ounce)"),
            currency_type: CurrencyType::CommodityBacked,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XPT() -> Self {
        *XPT_LOCK.get_or_init(|| Self {
            code: Ustr::from("XPT"),
            precision: 2,
            iso4217: 962,
            name: Ustr::from("Platinum (one troy ounce)"),
            currency_type: CurrencyType::CommodityBacked,
        })
    }

    ///////////////////////////////////////////////////////////////////////////
    // Crypto currencies
    ///////////////////////////////////////////////////////////////////////////
    #[allow(non_snake_case)]
    #[must_use]
    pub fn ONEINCH() -> Self {
        *ONEINCH_LOCK.get_or_init(|| Self {
            code: Ustr::from("1INCH"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("1inch Network"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn AAVE() -> Self {
        *AAVE_LOCK.get_or_init(|| Self {
            code: Ustr::from("AAVE"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Aave"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn ACA() -> Self {
        *ACA_LOCK.get_or_init(|| Self {
            code: Ustr::from("ACA"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Acala Token"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn ADA() -> Self {
        *ADA_LOCK.get_or_init(|| Self {
            code: Ustr::from("ADA"),
            precision: 6,
            iso4217: 0,
            name: Ustr::from("Cardano"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn AVAX() -> Self {
        *AVAX_LOCK.get_or_init(|| Self {
            code: Ustr::from("AVAX"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Avalanche"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BCH() -> Self {
        *BCH_LOCK.get_or_init(|| Self {
            code: Ustr::from("BCH"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Bitcoin Cash"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BTC() -> Self {
        *BTC_LOCK.get_or_init(|| Self {
            code: Ustr::from("BTC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Bitcoin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BTTC() -> Self {
        *BTTC_LOCK.get_or_init(|| Self {
            code: Ustr::from("BTTC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("BitTorrent"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BNB() -> Self {
        *BNB_LOCK.get_or_init(|| Self {
            code: Ustr::from("BNB"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Binance Coin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BRZ() -> Self {
        *BRZ_LOCK.get_or_init(|| Self {
            code: Ustr::from("BRZ"),
            precision: 6,
            iso4217: 0,
            name: Ustr::from("Brazilian Digital Token"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BSV() -> Self {
        *BSV_LOCK.get_or_init(|| Self {
            code: Ustr::from("BSV"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Bitcoin SV"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn BUSD() -> Self {
        *BUSD_LOCK.get_or_init(|| Self {
            code: Ustr::from("BUSD"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Binance USD"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn CAKE() -> Self {
        *CAKE_LOCK.get_or_init(|| Self {
            code: Ustr::from("CAKE"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("PancakeSwap"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn DASH() -> Self {
        *DASH_LOCK.get_or_init(|| Self {
            code: Ustr::from("DASH"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Dash"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn DOT() -> Self {
        *DOT_LOCK.get_or_init(|| Self {
            code: Ustr::from("DOT"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Polkadot"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn DOGE() -> Self {
        *DOGE_LOCK.get_or_init(|| Self {
            code: Ustr::from("DOGE"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Dogecoin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn EOS() -> Self {
        *EOS_LOCK.get_or_init(|| Self {
            code: Ustr::from("EOS"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("EOS"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn ETH() -> Self {
        *ETH_LOCK.get_or_init(|| Self {
            code: Ustr::from("ETH"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Ethereum"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn ETHW() -> Self {
        *ETHW_LOCK.get_or_init(|| Self {
            code: Ustr::from("ETHW"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("EthereumPoW"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn FDUSD() -> Self {
        *FDUSD_LOCK.get_or_init(|| Self {
            code: Ustr::from("FDUSD"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("First Digital USD"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn JOE() -> Self {
        *JOE_LOCK.get_or_init(|| Self {
            code: Ustr::from("JOE"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("JOE"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn LINK() -> Self {
        *LINK_LOCK.get_or_init(|| Self {
            code: Ustr::from("LINK"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Chainlink"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn LTC() -> Self {
        *LTC_LOCK.get_or_init(|| Self {
            code: Ustr::from("LTC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Litecoin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn LUNA() -> Self {
        *LUNA_LOCK.get_or_init(|| Self {
            code: Ustr::from("LUNA"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Terra"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn NBT() -> Self {
        *NBT_LOCK.get_or_init(|| Self {
            code: Ustr::from("NBT"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("NanoByte Token"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn SOL() -> Self {
        *SOL_LOCK.get_or_init(|| Self {
            code: Ustr::from("SOL"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Solana"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn SHIB() -> Self {
        *SHIB_LOCK.get_or_init(|| Self {
            code: Ustr::from("SHIB"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Shiba Inu"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn TRX() -> Self {
        *TRX_LOCK.get_or_init(|| Self {
            code: Ustr::from("TRX"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("TRON"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn TRYB() -> Self {
        *TRYB_LOCK.get_or_init(|| Self {
            code: Ustr::from("TRYB"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("BiLibra"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn TUSD() -> Self {
        *TUSD_LOCK.get_or_init(|| Self {
            code: Ustr::from("TUSD"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("TrueUSD"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn VTC() -> Self {
        *VTC_LOCK.get_or_init(|| Self {
            code: Ustr::from("VTC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Vertcoin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn WSB() -> Self {
        *WSB_LOCK.get_or_init(|| Self {
            code: Ustr::from("WSB"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("WallStreetBets DApp"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XBT() -> Self {
        *XBT_LOCK.get_or_init(|| Self {
            code: Ustr::from("XBT"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Bitcoin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XEC() -> Self {
        *XEC_LOCK.get_or_init(|| Self {
            code: Ustr::from("XEC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("eCash"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XLM() -> Self {
        *XLM_LOCK.get_or_init(|| Self {
            code: Ustr::from("XLM"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Stellar Lumen"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XMR() -> Self {
        *XMR_LOCK.get_or_init(|| Self {
            code: Ustr::from("XMR"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Monero"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn USDT() -> Self {
        *USDT_LOCK.get_or_init(|| Self {
            code: Ustr::from("USDT"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Tether"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XRP() -> Self {
        *XRP_LOCK.get_or_init(|| Self {
            code: Ustr::from("XRP"),
            precision: 6,
            iso4217: 0,
            name: Ustr::from("XRP"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn XTZ() -> Self {
        *XTZ_LOCK.get_or_init(|| Self {
            code: Ustr::from("XTZ"),
            precision: 6,
            iso4217: 0,
            name: Ustr::from("Tezos"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[must_use]
    #[allow(non_snake_case)]
    pub fn USDC() -> Self {
        *USDC_LOCK.get_or_init(|| Self {
            code: Ustr::from("USDC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("USD Coin"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[must_use]
    #[allow(non_snake_case)]
    pub fn USDC_POS() -> Self {
        *USDC_POS_LOCK.get_or_init(|| Self {
            code: Ustr::from("USDC.e"),
            precision: 6,
            iso4217: 0,
            name: Ustr::from("USD Coin (PoS)"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn USDP() -> Self {
        *USDP_LOCK.get_or_init(|| Self {
            code: Ustr::from("USDP"),
            precision: 4,
            iso4217: 0,
            name: Ustr::from("Pax Dollar"),
            currency_type: CurrencyType::Crypto,
        })
    }

    #[allow(non_snake_case)]
    #[must_use]
    pub fn ZEC() -> Self {
        *ZEC_LOCK.get_or_init(|| Self {
            code: Ustr::from("ZEC"),
            precision: 8,
            iso4217: 0,
            name: Ustr::from("Zcash"),
            currency_type: CurrencyType::Crypto,
        })
    }
}

/// A map of built-in `Currency` constants.
pub static CURRENCY_MAP: LazyLock<Mutex<HashMap<String, Currency>>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    ///////////////////////////////////////////////////////////////////////////
    // Fiat currencies
    ///////////////////////////////////////////////////////////////////////////
    map.insert(Currency::AUD().code.to_string(), Currency::AUD());
    map.insert(Currency::BRL().code.to_string(), Currency::BRL());
    map.insert(Currency::CAD().code.to_string(), Currency::CAD());
    map.insert(Currency::CHF().code.to_string(), Currency::CHF());
    map.insert(Currency::CNY().code.to_string(), Currency::CNY());
    map.insert(Currency::CNH().code.to_string(), Currency::CNH());
    map.insert(Currency::CZK().code.to_string(), Currency::CZK());
    map.insert(Currency::DKK().code.to_string(), Currency::DKK());
    map.insert(Currency::EUR().code.to_string(), Currency::EUR());
    map.insert(Currency::GBP().code.to_string(), Currency::GBP());
    map.insert(Currency::HKD().code.to_string(), Currency::HKD());
    map.insert(Currency::HUF().code.to_string(), Currency::HUF());
    map.insert(Currency::ILS().code.to_string(), Currency::ILS());
    map.insert(Currency::INR().code.to_string(), Currency::INR());
    map.insert(Currency::JPY().code.to_string(), Currency::JPY());
    map.insert(Currency::KRW().code.to_string(), Currency::KRW());
    map.insert(Currency::MXN().code.to_string(), Currency::MXN());
    map.insert(Currency::NOK().code.to_string(), Currency::NOK());
    map.insert(Currency::NZD().code.to_string(), Currency::NZD());
    map.insert(Currency::PLN().code.to_string(), Currency::PLN());
    map.insert(Currency::RUB().code.to_string(), Currency::RUB());
    map.insert(Currency::SAR().code.to_string(), Currency::SAR());
    map.insert(Currency::SEK().code.to_string(), Currency::SEK());
    map.insert(Currency::SGD().code.to_string(), Currency::SGD());
    map.insert(Currency::THB().code.to_string(), Currency::THB());
    map.insert(Currency::TRY().code.to_string(), Currency::TRY());
    map.insert(Currency::USD().code.to_string(), Currency::USD());
    map.insert(Currency::XAG().code.to_string(), Currency::XAG());
    map.insert(Currency::XAU().code.to_string(), Currency::XAU());
    map.insert(Currency::XPT().code.to_string(), Currency::XPT());
    map.insert(Currency::ZAR().code.to_string(), Currency::ZAR());
    ///////////////////////////////////////////////////////////////////////////
    // Crypto currencies
    ///////////////////////////////////////////////////////////////////////////
    map.insert(Currency::AAVE().code.to_string(), Currency::AAVE());
    map.insert(Currency::ACA().code.to_string(), Currency::ACA());
    map.insert(Currency::ADA().code.to_string(), Currency::ADA());
    map.insert(Currency::AVAX().code.to_string(), Currency::AVAX());
    map.insert(Currency::BCH().code.to_string(), Currency::BCH());
    map.insert(Currency::BTC().code.to_string(), Currency::BTC());
    map.insert(Currency::BTTC().code.to_string(), Currency::BTTC());
    map.insert(Currency::BNB().code.to_string(), Currency::BNB());
    map.insert(Currency::BRZ().code.to_string(), Currency::BRZ());
    map.insert(Currency::BSV().code.to_string(), Currency::BSV());
    map.insert(Currency::BUSD().code.to_string(), Currency::BUSD());
    map.insert(Currency::DASH().code.to_string(), Currency::DASH());
    map.insert(Currency::DOGE().code.to_string(), Currency::DOGE());
    map.insert(Currency::DOT().code.to_string(), Currency::DOT());
    map.insert(Currency::EOS().code.to_string(), Currency::EOS());
    map.insert(Currency::ETH().code.to_string(), Currency::ETH());
    map.insert(Currency::ETHW().code.to_string(), Currency::ETHW());
    map.insert(Currency::FDUSD().code.to_string(), Currency::FDUSD());
    map.insert(Currency::JOE().code.to_string(), Currency::JOE());
    map.insert(Currency::LINK().code.to_string(), Currency::LINK());
    map.insert(Currency::LTC().code.to_string(), Currency::LTC());
    map.insert(Currency::LUNA().code.to_string(), Currency::LUNA());
    map.insert(Currency::NBT().code.to_string(), Currency::NBT());
    map.insert(Currency::SOL().code.to_string(), Currency::SOL());
    map.insert(Currency::TRX().code.to_string(), Currency::TRX());
    map.insert(Currency::TRYB().code.to_string(), Currency::TRYB());
    map.insert(Currency::TUSD().code.to_string(), Currency::TUSD());
    map.insert(Currency::VTC().code.to_string(), Currency::VTC());
    map.insert(Currency::WSB().code.to_string(), Currency::WSB());
    map.insert(Currency::XBT().code.to_string(), Currency::XBT());
    map.insert(Currency::XEC().code.to_string(), Currency::XEC());
    map.insert(Currency::XLM().code.to_string(), Currency::XLM());
    map.insert(Currency::XMR().code.to_string(), Currency::XMR());
    map.insert(Currency::XRP().code.to_string(), Currency::XRP());
    map.insert(Currency::XTZ().code.to_string(), Currency::XTZ());
    map.insert(Currency::USDC().code.to_string(), Currency::USDC());
    map.insert(Currency::USDC_POS().code.to_string(), Currency::USDC_POS());
    map.insert(Currency::USDP().code.to_string(), Currency::USDP());
    map.insert(Currency::USDT().code.to_string(), Currency::USDT());
    map.insert(Currency::ZEC().code.to_string(), Currency::ZEC());
    Mutex::new(map)
});
