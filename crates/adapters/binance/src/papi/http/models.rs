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

//! Binance Portfolio Margin (PAPI) HTTP response models.
//!
//! Optional decimal fields distinguish missing, `null`, and empty inputs, which deserialize to
//! `None`, from an explicit `"0"` input, which deserializes to `Some(Decimal::ZERO)`.

use nautilus_core::serialization::deserialize_optional_decimal_from_str;
use rust_decimal::Decimal;
use serde::Deserialize;
use ustr::Ustr;

/// Response from `GET /papi/v1/balance`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(untagged)]
pub enum BinancePapiBalanceResponse {
    /// Balance entries for all assets.
    List(Vec<BinancePapiBalance>),
    /// Balance entry for one requested asset.
    Single(Box<BinancePapiBalance>),
}

/// Portfolio Margin balance entry for one asset.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePapiBalance {
    /// Asset code.
    pub asset: Ustr,
    /// Total wallet balance.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub total_wallet_balance: Option<Decimal>,
    /// Cross-Margin assets, equal to free plus locked.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_margin_asset: Option<Decimal>,
    /// Borrowed principal.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_margin_borrowed: Option<Decimal>,
    /// Free cross-Margin wallet amount.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_margin_free: Option<Decimal>,
    /// Cross-Margin interest.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_margin_interest: Option<Decimal>,
    /// Locked cross-Margin wallet amount.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_margin_locked: Option<Decimal>,
    /// USD-M wallet component.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub um_wallet_balance: Option<Decimal>,
    /// USD-M unrealized PnL.
    #[serde(
        rename = "umUnrealizedPNL",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub um_unrealized_pnl: Option<Decimal>,
    /// COIN-M wallet component.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cm_wallet_balance: Option<Decimal>,
    /// COIN-M unrealized PnL.
    #[serde(
        rename = "cmUnrealizedPNL",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub cm_unrealized_pnl: Option<Decimal>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
    /// Reported negative balance.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub negative_balance: Option<Decimal>,
}

/// Portfolio Margin account information from `GET /papi/v1/account`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePapiAccountInfo {
    /// Portfolio Margin maintenance margin rate.
    #[serde(
        rename = "uniMMR",
        default,
        deserialize_with = "deserialize_optional_decimal_from_str"
    )]
    pub uni_mmr: Option<Decimal>,
    /// Account equity in USD.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub account_equity: Option<Decimal>,
    /// Account equity without collateral rate in USD.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub actual_equity: Option<Decimal>,
    /// Account-level initial margin requirement.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub account_initial_margin: Option<Decimal>,
    /// Account-level maintenance margin requirement in USD.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub account_maint_margin: Option<Decimal>,
    /// Reported account status.
    #[serde(default)]
    pub account_status: Option<Ustr>,
    /// Maximum transfer-out capacity in USD.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub virtual_max_withdraw_amount: Option<Decimal>,
    /// Reported total available balance.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub total_available_balance: Option<Decimal>,
    /// Reported total Margin open-order loss in USD.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub total_margin_open_loss: Option<Decimal>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
}

/// Shared USD-M or COIN-M account asset details.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePapiFuturesAsset {
    /// Asset code.
    pub asset: Ustr,
    /// Cross wallet balance.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_wallet_balance: Option<Decimal>,
    /// Cross unrealized PnL.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub cross_un_pnl: Option<Decimal>,
    /// Maintenance margin requirement.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub maint_margin: Option<Decimal>,
    /// Total initial margin requirement with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub initial_margin: Option<Decimal>,
    /// Initial margin requirement for positions with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub position_initial_margin: Option<Decimal>,
    /// Initial margin requirement for open orders with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub open_order_initial_margin: Option<Decimal>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
}

/// USD-M account details from `GET /papi/v1/um/account`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BinancePapiUmAccount {
    /// Account assets.
    #[serde(default)]
    pub assets: Option<Vec<BinancePapiFuturesAsset>>,
    /// Positions for all symbols returned by the venue.
    #[serde(default)]
    pub positions: Option<Vec<BinancePapiUmPosition>>,
}

/// USD-M account details from `GET /papi/v2/um/account`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BinancePapiUmAccountV2 {
    /// Account assets.
    #[serde(default)]
    pub assets: Option<Vec<BinancePapiFuturesAsset>>,
    /// Positions for all symbols returned by the venue.
    #[serde(default)]
    pub positions: Option<Vec<BinancePapiUmPositionV2>>,
}

/// COIN-M account details from `GET /papi/v1/cm/account`.
#[derive(Clone, Debug, PartialEq, Deserialize)]
pub struct BinancePapiCmAccount {
    /// Account assets.
    #[serde(default)]
    pub assets: Option<Vec<BinancePapiFuturesAsset>>,
    /// Positions for all symbols returned by the venue.
    #[serde(default)]
    pub positions: Option<Vec<BinancePapiCmPosition>>,
}

/// USD-M V1 position details.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePapiUmPosition {
    /// Symbol name.
    pub symbol: Ustr,
    /// Total initial margin requirement with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub initial_margin: Option<Decimal>,
    /// Maintenance margin requirement.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub maint_margin: Option<Decimal>,
    /// Unrealized profit.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub unrealized_profit: Option<Decimal>,
    /// Initial margin requirement for the position with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub position_initial_margin: Option<Decimal>,
    /// Initial margin requirement for open orders with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub open_order_initial_margin: Option<Decimal>,
    /// Current initial leverage.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub leverage: Option<Decimal>,
    /// Average entry price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub entry_price: Option<Decimal>,
    /// Maximum available notional with the current leverage.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub max_notional: Option<Decimal>,
    /// Bid notional; Binance documents this as ignore.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub bid_notional: Option<Decimal>,
    /// Ask notional; Binance documents this as ignore.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub ask_notional: Option<Decimal>,
    /// Position side reported by the venue.
    pub position_side: Ustr,
    /// Signed position amount.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub position_amt: Option<Decimal>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
}

/// USD-M V2 position details.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePapiUmPositionV2 {
    /// Symbol name.
    pub symbol: Ustr,
    /// Total initial margin requirement with the current mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub initial_margin: Option<Decimal>,
    /// Maintenance margin requirement.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub maint_margin: Option<Decimal>,
    /// Unrealized profit.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub unrealized_profit: Option<Decimal>,
    /// Position side reported by the venue.
    pub position_side: Ustr,
    /// Signed position amount.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub position_amt: Option<Decimal>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
    /// Position notional.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub notional: Option<Decimal>,
}

/// COIN-M position details.
#[derive(Clone, Debug, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinancePapiCmPosition {
    /// Symbol name.
    pub symbol: Ustr,
    /// Signed position amount.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub position_amt: Option<Decimal>,
    /// Total initial margin requirement with the latest mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub initial_margin: Option<Decimal>,
    /// Maintenance margin requirement.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub maint_margin: Option<Decimal>,
    /// Unrealized profit or loss.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub unrealized_profit: Option<Decimal>,
    /// Initial margin requirement for the position with the latest mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub position_initial_margin: Option<Decimal>,
    /// Initial margin requirement for open orders with the latest mark price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub open_order_initial_margin: Option<Decimal>,
    /// Current initial leverage.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub leverage: Option<Decimal>,
    /// Position side reported by the venue.
    pub position_side: Ustr,
    /// Average entry price.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub entry_price: Option<Decimal>,
    /// Maximum quantity of the base asset.
    #[serde(default, deserialize_with = "deserialize_optional_decimal_from_str")]
    pub max_qty: Option<Decimal>,
    /// Last update time in milliseconds.
    #[serde(default)]
    pub update_time: Option<i64>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_balance_list_response_decodes_all_fields() {
        let response: BinancePapiBalanceResponse = serde_json::from_str(include_str!(
            "../../../test_data/papi/http_json/balance_list_response.json"
        ))
        .unwrap();

        let BinancePapiBalanceResponse::List(balances) = response else {
            panic!("expected list response");
        };

        assert_eq!(balances.len(), 2);

        let usdt = &balances[0];
        assert_eq!(usdt.asset, Ustr::from("USDT"));
        assert_eq!(
            usdt.total_wallet_balance,
            Some(dec!(250.125000000000000001))
        );
        assert_eq!(usdt.cross_margin_asset, Some(dec!(140.500000000000000002)));
        assert_eq!(usdt.cross_margin_borrowed, Some(dec!(12.25)));
        assert_eq!(usdt.cross_margin_free, Some(dec!(125.25)));
        assert_eq!(usdt.cross_margin_interest, Some(dec!(0.75)));
        assert_eq!(usdt.cross_margin_locked, Some(dec!(15.25)));
        assert_eq!(usdt.um_wallet_balance, Some(dec!(80.5)));
        assert_eq!(usdt.um_unrealized_pnl, Some(dec!(-2.5)));
        assert_eq!(usdt.cm_wallet_balance, Some(dec!(25.25)));
        assert_eq!(usdt.cm_unrealized_pnl, Some(Decimal::ZERO));
        assert_eq!(usdt.update_time, Some(1_700_000_000_123));
        assert_eq!(usdt.negative_balance, Some(Decimal::ZERO));

        let btc = &balances[1];
        assert_eq!(btc.asset, Ustr::from("BTC"));
        assert_eq!(btc.total_wallet_balance, Some(dec!(0.050000000000000001)));
        assert_eq!(btc.cross_margin_asset, Some(dec!(0.020000000000000001)));
        assert_eq!(btc.cross_margin_borrowed, Some(dec!(0.005)));
        assert_eq!(btc.cross_margin_free, Some(dec!(0.015)));
        assert_eq!(btc.cross_margin_interest, Some(dec!(0.0001)));
        assert_eq!(btc.cross_margin_locked, Some(dec!(0.005)));
        assert_eq!(btc.um_wallet_balance, Some(dec!(0.01)));
        assert_eq!(btc.um_unrealized_pnl, None);
        assert_eq!(btc.cm_wallet_balance, Some(dec!(0.02)));
        assert_eq!(btc.cm_unrealized_pnl, Some(dec!(-0.0002)));
        assert_eq!(btc.update_time, Some(1_700_000_000_456));
        assert_eq!(btc.negative_balance, Some(dec!(-0.003)));
    }

    #[rstest]
    fn test_balance_single_response_distinguishes_missing_empty_and_zero() {
        let response: BinancePapiBalanceResponse = serde_json::from_str(include_str!(
            "../../../test_data/papi/http_json/balance_single_response.json"
        ))
        .unwrap();

        let BinancePapiBalanceResponse::Single(balance) = response else {
            panic!("expected single response");
        };

        assert_eq!(balance.asset, Ustr::from("USDT"));
        assert_eq!(balance.total_wallet_balance, Some(dec!(42.00000001)));
        assert_eq!(balance.cross_margin_asset, None);
        assert_eq!(balance.cross_margin_borrowed, Some(Decimal::ZERO));
        assert_eq!(balance.cross_margin_free, Some(dec!(40.00000001)));
        assert_eq!(balance.cross_margin_interest, None);
        assert_eq!(balance.cross_margin_locked, Some(dec!(2)));
        assert_eq!(balance.um_wallet_balance, None);
        assert_eq!(balance.um_unrealized_pnl, None);
        assert_eq!(balance.cm_wallet_balance, Some(Decimal::ZERO));
        assert_eq!(balance.cm_unrealized_pnl, Some(Decimal::ZERO));
        assert_eq!(balance.update_time, Some(1_700_000_000_789));
        assert_eq!(balance.negative_balance, None);
    }

    #[rstest]
    fn test_balance_invalid_decimal_returns_error() {
        let error = serde_json::from_str::<BinancePapiBalance>(include_str!(
            "../../../test_data/papi/http_json/balance_invalid_decimal_response.json"
        ))
        .unwrap_err();

        assert!(error.to_string().contains("Invalid decimal"), "{error}");

        let error = serde_json::from_str::<BinancePapiBalanceResponse>(include_str!(
            "../../../test_data/papi/http_json/balance_invalid_decimal_response.json"
        ))
        .unwrap_err();

        assert!(
            error.to_string().contains("did not match any variant"),
            "{error}"
        );
    }

    #[rstest]
    fn test_account_information_decodes_all_fields() {
        let account: BinancePapiAccountInfo = serde_json::from_str(include_str!(
            "../../../test_data/papi/http_json/account_information_response.json"
        ))
        .unwrap();

        assert_eq!(account.uni_mmr, Some(dec!(0.123456789012345678)));
        assert_eq!(account.account_equity, Some(dec!(1000.000000000000000001)));
        assert_eq!(account.actual_equity, Some(dec!(1250.25)));
        assert_eq!(account.account_initial_margin, Some(dec!(200.5)));
        assert_eq!(account.account_maint_margin, Some(dec!(150.25)));
        assert_eq!(account.account_status, Some(Ustr::from("MARGIN_CALL")));
        assert_eq!(account.virtual_max_withdraw_amount, Some(dec!(300.75)));
        assert_eq!(account.total_available_balance, None);
        assert_eq!(account.total_margin_open_loss, Some(Decimal::ZERO));
        assert_eq!(account.update_time, Some(1_700_000_001_000));
    }

    #[rstest]
    fn test_um_account_v1_decodes_assets_and_positions() {
        let account: BinancePapiUmAccount = serde_json::from_str(include_str!(
            "../../../test_data/papi/http_json/um_account_detail_v1_response.json"
        ))
        .unwrap();
        let assets = account.assets.unwrap();
        let positions = account.positions.unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(positions.len(), 1);

        let asset = &assets[0];
        assert_eq!(asset.asset, Ustr::from("USDT"));
        assert_eq!(
            asset.cross_wallet_balance,
            Some(dec!(500.000000000000000001))
        );
        assert_eq!(asset.cross_un_pnl, Some(dec!(-2.5)));
        assert_eq!(asset.maint_margin, Some(dec!(10.5)));
        assert_eq!(asset.initial_margin, Some(dec!(12.5)));
        assert_eq!(asset.position_initial_margin, Some(dec!(8.5)));
        assert_eq!(asset.open_order_initial_margin, Some(dec!(4)));
        assert_eq!(asset.update_time, Some(1_700_000_002_000));

        let position = &positions[0];
        assert_eq!(position.symbol, Ustr::from("BTCUSDT"));
        assert_eq!(position.initial_margin, Some(dec!(12.5)));
        assert_eq!(position.maint_margin, Some(dec!(10.5)));
        assert_eq!(position.unrealized_profit, Some(dec!(-1.25)));
        assert_eq!(position.position_initial_margin, Some(dec!(8.5)));
        assert_eq!(position.open_order_initial_margin, Some(dec!(4)));
        assert_eq!(position.leverage, Some(dec!(20)));
        assert_eq!(position.entry_price, Some(dec!(50000.5)));
        assert_eq!(position.max_notional, Some(dec!(100000)));
        assert_eq!(position.bid_notional, Some(dec!(100.5)));
        assert_eq!(position.ask_notional, Some(dec!(101.5)));
        assert_eq!(position.position_side, Ustr::from("SHORT"));
        assert_eq!(position.position_amt, Some(dec!(-0.025)));
        assert_eq!(position.update_time, Some(1_700_000_002_001));
    }

    #[rstest]
    fn test_um_account_v2_decodes_assets_and_positions() {
        let account: BinancePapiUmAccountV2 = serde_json::from_str(include_str!(
            "../../../test_data/papi/http_json/um_account_detail_v2_response.json"
        ))
        .unwrap();
        let assets = account.assets.unwrap();
        let positions = account.positions.unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(positions.len(), 1);

        let asset = &assets[0];
        assert_eq!(asset.asset, Ustr::from("USDT"));
        assert_eq!(asset.cross_wallet_balance, Some(dec!(600.25)));
        assert_eq!(asset.cross_un_pnl, Some(dec!(2.25)));
        assert_eq!(asset.maint_margin, Some(dec!(6.5)));
        assert_eq!(asset.initial_margin, Some(dec!(7.5)));
        assert_eq!(asset.position_initial_margin, Some(dec!(5.5)));
        assert_eq!(asset.open_order_initial_margin, Some(dec!(2)));
        assert_eq!(asset.update_time, Some(1_700_000_003_000));

        let position = &positions[0];
        assert_eq!(position.symbol, Ustr::from("ETHUSDT"));
        assert_eq!(position.initial_margin, Some(dec!(7.5)));
        assert_eq!(position.maint_margin, Some(dec!(6.5)));
        assert_eq!(position.unrealized_profit, Some(dec!(2.25)));
        assert_eq!(position.position_side, Ustr::from("LONG"));
        assert_eq!(position.position_amt, Some(dec!(1.5)));
        assert_eq!(position.update_time, Some(1_700_000_003_001));
        assert_eq!(position.notional, Some(dec!(7500.25)));
    }

    #[rstest]
    fn test_cm_account_decodes_assets_and_positions() {
        let account: BinancePapiCmAccount = serde_json::from_str(include_str!(
            "../../../test_data/papi/http_json/cm_account_detail_response.json"
        ))
        .unwrap();
        let assets = account.assets.unwrap();
        let positions = account.positions.unwrap();
        assert_eq!(assets.len(), 1);
        assert_eq!(positions.len(), 1);

        let asset = &assets[0];
        assert_eq!(asset.asset, Ustr::from("BTC"));
        assert_eq!(asset.cross_wallet_balance, Some(dec!(0.050000000000000001)));
        assert_eq!(asset.cross_un_pnl, Some(dec!(-0.0002)));
        assert_eq!(asset.maint_margin, Some(dec!(0.0004)));
        assert_eq!(asset.initial_margin, Some(dec!(0.0005)));
        assert_eq!(asset.position_initial_margin, Some(dec!(0.0003)));
        assert_eq!(asset.open_order_initial_margin, Some(dec!(0.0002)));
        assert_eq!(asset.update_time, Some(1_700_000_004_000));

        let position = &positions[0];
        assert_eq!(position.symbol, Ustr::from("BTCUSD_261225"));
        assert_eq!(position.position_amt, Some(dec!(-15)));
        assert_eq!(position.initial_margin, Some(dec!(0.0005)));
        assert_eq!(position.maint_margin, Some(dec!(0.0004)));
        assert_eq!(position.unrealized_profit, Some(dec!(-0.00001)));
        assert_eq!(position.position_initial_margin, Some(dec!(0.0003)));
        assert_eq!(position.open_order_initial_margin, Some(dec!(0.0002)));
        assert_eq!(position.leverage, Some(dec!(5)));
        assert_eq!(position.position_side, Ustr::from("SHORT"));
        assert_eq!(position.entry_price, Some(dec!(60000.5)));
        assert_eq!(position.max_qty, Some(dec!(100)));
        assert_eq!(position.update_time, Some(1_700_000_004_001));
    }
}
