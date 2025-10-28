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

//! Data structures representing BitMEX REST API payloads.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use ustr::Ustr;
use uuid::Uuid;

use crate::common::enums::{
    BitmexContingencyType, BitmexExecInstruction, BitmexExecType, BitmexFairMethod,
    BitmexInstrumentState, BitmexInstrumentType, BitmexLiquidityIndicator, BitmexMarkMethod,
    BitmexOrderStatus, BitmexOrderType, BitmexPegPriceType, BitmexSide, BitmexTickDirection,
    BitmexTimeInForce,
};

/// Custom deserializer for comma-separated `ExecInstruction` values
fn deserialize_exec_instructions<'de, D>(
    deserializer: D,
) -> Result<Option<Vec<BitmexExecInstruction>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let s: Option<String> = Option::deserialize(deserializer)?;
    match s {
        None => Ok(None),
        Some(ref s) if s.is_empty() => Ok(None),
        Some(s) => {
            let instructions: Result<Vec<BitmexExecInstruction>, _> = s
                .split(',')
                .map(|inst| {
                    let trimmed = inst.trim();
                    match trimmed {
                        "ParticipateDoNotInitiate" => {
                            Ok(BitmexExecInstruction::ParticipateDoNotInitiate)
                        }
                        "AllOrNone" => Ok(BitmexExecInstruction::AllOrNone),
                        "MarkPrice" => Ok(BitmexExecInstruction::MarkPrice),
                        "IndexPrice" => Ok(BitmexExecInstruction::IndexPrice),
                        "LastPrice" => Ok(BitmexExecInstruction::LastPrice),
                        "Close" => Ok(BitmexExecInstruction::Close),
                        "ReduceOnly" => Ok(BitmexExecInstruction::ReduceOnly),
                        "Fixed" => Ok(BitmexExecInstruction::Fixed),
                        "" => Ok(BitmexExecInstruction::Unknown),
                        _ => Err(serde::de::Error::custom(format!(
                            "Unknown ExecInstruction: {trimmed}"
                        ))),
                    }
                })
                .collect();
            instructions.map(Some)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexInstrument {
    pub symbol: Ustr,
    pub root_symbol: Ustr,
    pub state: BitmexInstrumentState,
    #[serde(rename = "typ")]
    pub instrument_type: BitmexInstrumentType,
    pub listing: Option<DateTime<Utc>>,
    pub front: Option<DateTime<Utc>>,
    pub expiry: Option<DateTime<Utc>>,
    pub settle: Option<DateTime<Utc>>,
    pub listed_settle: Option<DateTime<Utc>>,
    pub position_currency: Option<Ustr>,
    pub underlying: Ustr,
    pub quote_currency: Ustr,
    pub underlying_symbol: Option<Ustr>,
    pub reference: Option<Ustr>,
    pub reference_symbol: Option<Ustr>,
    pub calc_interval: Option<DateTime<Utc>>,
    pub publish_interval: Option<DateTime<Utc>>,
    pub publish_time: Option<DateTime<Utc>>,
    pub max_order_qty: Option<f64>,
    pub max_price: Option<f64>,
    pub lot_size: Option<f64>,
    pub tick_size: f64,
    pub multiplier: f64,
    pub settl_currency: Option<Ustr>,
    pub underlying_to_position_multiplier: Option<f64>,
    pub underlying_to_settle_multiplier: Option<f64>,
    pub quote_to_settle_multiplier: Option<f64>,
    pub is_quanto: bool,
    pub is_inverse: bool,
    pub init_margin: Option<f64>,
    pub maint_margin: Option<f64>,
    pub risk_limit: Option<f64>,
    pub risk_step: Option<f64>,
    pub limit: Option<f64>,
    pub taxed: Option<bool>,
    pub deleverage: Option<bool>,
    pub maker_fee: Option<f64>,
    pub taker_fee: Option<f64>,
    pub settlement_fee: Option<f64>,
    pub funding_base_symbol: Option<Ustr>,
    pub funding_quote_symbol: Option<Ustr>,
    pub funding_premium_symbol: Option<Ustr>,
    pub funding_timestamp: Option<DateTime<Utc>>,
    pub funding_interval: Option<DateTime<Utc>>,
    pub funding_rate: Option<f64>,
    pub indicative_funding_rate: Option<f64>,
    pub rebalance_timestamp: Option<DateTime<Utc>>,
    pub rebalance_interval: Option<DateTime<Utc>>,
    pub prev_close_price: Option<f64>,
    pub limit_down_price: Option<f64>,
    pub limit_up_price: Option<f64>,
    pub prev_total_volume: Option<f64>,
    pub total_volume: Option<f64>,
    pub volume: Option<f64>,
    #[serde(rename = "volume24h")]
    pub volume_24h: Option<f64>,
    pub prev_total_turnover: Option<f64>,
    pub total_turnover: Option<f64>,
    pub turnover: Option<f64>,
    #[serde(rename = "turnover24h")]
    pub turnover_24h: Option<f64>,
    #[serde(rename = "homeNotional24h")]
    pub home_notional_24h: Option<f64>,
    #[serde(rename = "foreignNotional24h")]
    pub foreign_notional_24h: Option<f64>,
    #[serde(rename = "prevPrice24h")]
    pub prev_price_24h: Option<f64>,
    pub vwap: Option<f64>,
    pub high_price: Option<f64>,
    pub low_price: Option<f64>,
    pub last_price: Option<f64>,
    pub last_price_protected: Option<f64>,
    pub last_tick_direction: Option<BitmexTickDirection>,
    pub last_change_pcnt: Option<f64>,
    pub bid_price: Option<f64>,
    pub mid_price: Option<f64>,
    pub ask_price: Option<f64>,
    pub impact_bid_price: Option<f64>,
    pub impact_mid_price: Option<f64>,
    pub impact_ask_price: Option<f64>,
    pub has_liquidity: Option<bool>,
    pub open_interest: Option<f64>,
    pub open_value: Option<f64>,
    pub fair_method: Option<BitmexFairMethod>,
    pub fair_basis_rate: Option<f64>,
    pub fair_basis: Option<f64>,
    pub fair_price: Option<f64>,
    pub mark_method: Option<BitmexMarkMethod>,
    pub mark_price: Option<f64>,
    pub indicative_settle_price: Option<f64>,
    pub settled_price_adjustment_rate: Option<f64>,
    pub settled_price: Option<f64>,
    pub instant_pnl: bool,
    pub min_tick: Option<f64>,
    pub funding_base_rate: Option<f64>,
    pub funding_quote_rate: Option<f64>,
    pub capped: Option<bool>,
    pub opening_timestamp: Option<DateTime<Utc>>,
    pub closing_timestamp: Option<DateTime<Utc>>,
    pub timestamp: DateTime<Utc>,
}

/// Raw Order and Balance Data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexExecution {
    #[serde(rename = "execID")]
    pub exec_id: Uuid,
    #[serde(rename = "orderID")]
    pub order_id: Option<Uuid>,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<Ustr>,
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<Ustr>,
    pub account: i64,
    pub symbol: Option<Ustr>,
    pub side: Option<BitmexSide>,
    pub last_qty: i64,
    pub last_px: f64,
    pub underlying_last_px: Option<f64>,
    pub last_mkt: Option<Ustr>,
    pub last_liquidity_ind: Option<BitmexLiquidityIndicator>,
    pub order_qty: Option<i64>,
    pub price: Option<f64>,
    pub display_qty: Option<i64>,
    pub stop_px: Option<f64>,
    pub peg_offset_value: Option<f64>,
    pub peg_price_type: Option<BitmexPegPriceType>,
    pub currency: Option<Ustr>,
    pub settl_currency: Option<Ustr>,
    pub exec_type: BitmexExecType,
    pub ord_type: BitmexOrderType,
    pub time_in_force: BitmexTimeInForce,
    #[serde(default, deserialize_with = "deserialize_exec_instructions")]
    pub exec_inst: Option<Vec<BitmexExecInstruction>>,
    pub contingency_type: Option<BitmexContingencyType>,
    pub ex_destination: Option<Ustr>,
    pub ord_status: Option<BitmexOrderStatus>,
    pub triggered: Option<Ustr>,
    pub working_indicator: Option<bool>,
    pub ord_rej_reason: Option<Ustr>,
    pub leaves_qty: Option<i64>,
    pub cum_qty: Option<i64>,
    pub avg_px: Option<f64>,
    pub commission: Option<f64>,
    pub trade_publish_indicator: Option<Ustr>,
    pub multi_leg_reporting_type: Option<Ustr>,
    pub text: Option<Ustr>,
    pub trd_match_id: Option<Uuid>,
    pub exec_cost: Option<i64>,
    pub exec_comm: Option<i64>,
    pub home_notional: Option<f64>,
    pub foreign_notional: Option<f64>,
    pub transact_time: Option<DateTime<Utc>>,
    pub timestamp: Option<DateTime<Utc>>,
}

/// Swap Funding History.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexFunding {
    pub timestamp: DateTime<Utc>,
    pub symbol: Ustr,
    pub funding_interval: Option<DateTime<Utc>>,
    pub funding_rate: Option<f64>,
    pub funding_rate_daily: Option<f64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitmexInstrumentInterval {
    pub intervals: Vec<String>,
    pub symbols: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexIndexComposite {
    pub timestamp: DateTime<Utc>,
    pub symbol: Option<String>,
    pub index_symbol: Option<String>,
    pub reference: Option<String>,
    pub last_price: Option<f64>,
    pub weight: Option<f64>,
    pub logged: Option<DateTime<Utc>>,
}

/// Insurance Fund Data.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexInsurance {
    pub currency: Ustr,
    pub timestamp: DateTime<Utc>,
    pub wallet_balance: Option<i64>,
}

/// Active Liquidations.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexLiquidation {
    #[serde(rename = "orderID")]
    pub order_id: Uuid,
    pub symbol: Option<String>,
    pub side: Option<BitmexSide>,
    pub price: Option<f64>,
    pub leaves_qty: Option<i64>,
}

/// Placement, Cancellation, Amending, and History.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexOrder {
    #[serde(rename = "orderID")]
    pub order_id: Uuid,
    #[serde(rename = "clOrdID")]
    pub cl_ord_id: Option<Ustr>,
    #[serde(rename = "clOrdLinkID")]
    pub cl_ord_link_id: Option<Ustr>,
    pub account: i64,
    pub symbol: Option<Ustr>,
    pub side: Option<BitmexSide>,
    pub order_qty: Option<i64>,
    pub price: Option<f64>,
    pub display_qty: Option<i64>,
    pub stop_px: Option<f64>,
    pub peg_offset_value: Option<f64>,
    pub peg_price_type: Option<BitmexPegPriceType>,
    pub currency: Option<Ustr>,
    pub settl_currency: Option<Ustr>,
    pub ord_type: Option<BitmexOrderType>,
    pub time_in_force: Option<BitmexTimeInForce>,
    #[serde(default, deserialize_with = "deserialize_exec_instructions")]
    pub exec_inst: Option<Vec<BitmexExecInstruction>>,
    pub contingency_type: Option<BitmexContingencyType>,
    pub ex_destination: Option<Ustr>,
    pub ord_status: Option<BitmexOrderStatus>,
    pub triggered: Option<Ustr>,
    pub working_indicator: Option<bool>,
    pub ord_rej_reason: Option<Ustr>,
    pub leaves_qty: Option<i64>,
    pub cum_qty: Option<i64>,
    pub avg_px: Option<f64>,
    pub multi_leg_reporting_type: Option<Ustr>,
    pub text: Option<Ustr>,
    pub transact_time: Option<DateTime<Utc>>,
    pub timestamp: Option<DateTime<Utc>>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexOrderBookL2 {
    pub symbol: Ustr,
    pub id: i64,
    pub side: BitmexSide,
    pub size: Option<i64>,
    pub price: Option<f64>,
}

/// Position status.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexPosition {
    pub account: i64,
    pub symbol: Ustr,
    pub currency: Option<Ustr>,
    pub underlying: Option<Ustr>,
    pub quote_currency: Option<Ustr>,
    pub commission: Option<f64>,
    pub init_margin_req: Option<f64>,
    pub maint_margin_req: Option<f64>,
    pub risk_limit: Option<i64>,
    pub leverage: Option<f64>,
    pub cross_margin: Option<bool>,
    pub deleverage_percentile: Option<f64>,
    pub rebalanced_pnl: Option<i64>,
    pub prev_realised_pnl: Option<i64>,
    pub prev_unrealised_pnl: Option<i64>,
    pub prev_close_price: Option<f64>,
    pub opening_timestamp: Option<DateTime<Utc>>,
    pub opening_qty: Option<i64>,
    pub opening_cost: Option<i64>,
    pub opening_comm: Option<i64>,
    pub open_order_buy_qty: Option<i64>,
    pub open_order_buy_cost: Option<i64>,
    pub open_order_buy_premium: Option<i64>,
    pub open_order_sell_qty: Option<i64>,
    pub open_order_sell_cost: Option<i64>,
    pub open_order_sell_premium: Option<i64>,
    pub exec_buy_qty: Option<i64>,
    pub exec_buy_cost: Option<i64>,
    pub exec_sell_qty: Option<i64>,
    pub exec_sell_cost: Option<i64>,
    pub exec_qty: Option<i64>,
    pub exec_cost: Option<i64>,
    pub exec_comm: Option<i64>,
    pub current_timestamp: Option<DateTime<Utc>>,
    pub current_qty: Option<i64>,
    pub current_cost: Option<i64>,
    pub current_comm: Option<i64>,
    pub realised_cost: Option<i64>,
    pub unrealised_cost: Option<i64>,
    pub gross_open_cost: Option<i64>,
    pub gross_open_premium: Option<i64>,
    pub gross_exec_cost: Option<i64>,
    pub is_open: Option<bool>,
    pub mark_price: Option<f64>,
    pub mark_value: Option<i64>,
    pub risk_value: Option<i64>,
    pub home_notional: Option<f64>,
    pub foreign_notional: Option<f64>,
    pub pos_state: Option<Ustr>,
    pub pos_cost: Option<i64>,
    pub pos_cost2: Option<i64>,
    pub pos_cross: Option<i64>,
    pub pos_init: Option<i64>,
    pub pos_comm: Option<i64>,
    pub pos_loss: Option<i64>,
    pub pos_margin: Option<i64>,
    pub pos_maint: Option<i64>,
    pub pos_allowance: Option<i64>,
    pub taxable_margin: Option<i64>,
    pub init_margin: Option<i64>,
    pub maint_margin: Option<i64>,
    pub session_margin: Option<i64>,
    pub target_excess_margin: Option<i64>,
    pub var_margin: Option<i64>,
    pub realised_gross_pnl: Option<i64>,
    pub realised_tax: Option<i64>,
    pub realised_pnl: Option<i64>,
    pub unrealised_gross_pnl: Option<i64>,
    pub long_bankrupt: Option<i64>,
    pub short_bankrupt: Option<i64>,
    pub tax_base: Option<i64>,
    pub indicative_tax_rate: Option<f64>,
    pub indicative_tax: Option<i64>,
    pub unrealised_tax: Option<i64>,
    pub unrealised_pnl: Option<i64>,
    pub unrealised_pnl_pcnt: Option<f64>,
    pub unrealised_roe_pcnt: Option<f64>,
    pub avg_cost_price: Option<f64>,
    pub avg_entry_price: Option<f64>,
    pub break_even_price: Option<f64>,
    pub margin_call_price: Option<f64>,
    pub liquidation_price: Option<f64>,
    pub bankrupt_price: Option<f64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub last_price: Option<f64>,
    pub last_value: Option<i64>,
}

/// Best Bid/Offer Snapshots & Historical Bins.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexQuote {
    pub timestamp: DateTime<Utc>,
    pub symbol: Ustr,
    pub bid_size: Option<i64>,
    pub bid_price: Option<f64>,
    pub ask_price: Option<f64>,
    pub ask_size: Option<i64>,
}

/// Historical Settlement Data.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexSettlement {
    pub timestamp: DateTime<Utc>,
    pub symbol: Ustr,
    pub settlement_type: Option<String>,
    pub settled_price: Option<f64>,
    pub option_strike_price: Option<f64>,
    pub option_underlying_price: Option<f64>,
    pub bankrupt: Option<i64>,
    pub tax_base: Option<i64>,
    pub tax_rate: Option<f64>,
}

/// Exchange Statistics.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexStats {
    pub root_symbol: Ustr,
    pub currency: Option<String>,
    pub volume24h: Option<i64>,
    pub turnover24h: Option<i64>,
    pub open_interest: Option<i64>,
    pub open_value: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexStatsHistory {
    pub date: DateTime<Utc>,
    pub root_symbol: Ustr,
    pub currency: Option<String>,
    pub volume: Option<i64>,
    pub turnover: Option<i64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexStatsUSD {
    pub root_symbol: Ustr,
    pub currency: Option<String>,
    pub turnover24h: Option<i64>,
    pub turnover30d: Option<i64>,
    pub turnover365d: Option<i64>,
    pub turnover: Option<i64>,
}

/// Individual & Bucketed Trades.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexTrade {
    pub timestamp: DateTime<Utc>,
    pub symbol: Ustr,
    pub side: Option<BitmexSide>,
    pub size: i64,
    pub price: f64,
    pub tick_direction: Option<String>,
    #[serde(rename = "trdMatchID")]
    pub trd_match_id: Option<Uuid>,
    pub gross_value: Option<i64>,
    pub home_notional: Option<f64>,
    pub foreign_notional: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexTradeBin {
    pub timestamp: DateTime<Utc>,
    pub symbol: Ustr,
    pub open: Option<f64>,
    pub high: Option<f64>,
    pub low: Option<f64>,
    pub close: Option<f64>,
    pub trades: Option<i64>,
    pub volume: Option<i64>,
    pub vwap: Option<f64>,
    pub last_size: Option<i64>,
    pub turnover: Option<i64>,
    pub home_notional: Option<f64>,
    pub foreign_notional: Option<f64>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexWallet {
    pub account: i64,
    pub currency: Ustr,
    pub prev_deposited: Option<i64>,
    pub prev_withdrawn: Option<i64>,
    pub prev_transfer_in: Option<i64>,
    pub prev_transfer_out: Option<i64>,
    pub prev_amount: Option<i64>,
    pub prev_timestamp: Option<DateTime<Utc>>,
    pub delta_deposited: Option<i64>,
    pub delta_withdrawn: Option<i64>,
    pub delta_transfer_in: Option<i64>,
    pub delta_transfer_out: Option<i64>,
    pub delta_amount: Option<i64>,
    pub deposited: Option<i64>,
    pub withdrawn: Option<i64>,
    pub transfer_in: Option<i64>,
    pub transfer_out: Option<i64>,
    pub amount: Option<i64>,
    pub pending_credit: Option<i64>,
    pub pending_debit: Option<i64>,
    pub confirmed_debit: Option<i64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub addr: Option<Ustr>,
    pub script: Option<Ustr>,
    pub withdrawal_lock: Option<Vec<Ustr>>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitmexTransaction {
    pub transact_id: Option<Uuid>,
    pub account: Option<i64>,
    pub currency: Option<Ustr>,
    pub transact_type: Option<Ustr>,
    pub amount: Option<i64>,
    pub fee: Option<i64>,
    pub transact_status: Option<Ustr>,
    pub address: Option<Ustr>,
    pub tx: Option<Ustr>,
    pub text: Option<Ustr>,
    pub transact_time: Option<DateTime<Utc>>,
    pub timestamp: Option<DateTime<Utc>>,
}

/// Public Announcements.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexAnnouncement {
    pub id: i32,
    pub link: Option<String>,
    pub title: Option<String>,
    pub content: Option<String>,
    pub date: Option<DateTime<Utc>>,
}

/// Persistent API Keys for Developers.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexAPIKey {
    pub id: String,
    pub secret: Option<String>,
    pub name: String,
    pub nonce: i64,
    pub cidr: Option<String>,
    pub permissions: Vec<serde_json::Value>,
    pub enabled: Option<bool>,
    pub user_id: i32,
    pub created: Option<DateTime<Utc>>,
}

/// Account Notifications.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexGlobalNotification {
    pub id: Option<i32>,
    pub date: DateTime<Utc>,
    pub title: String,
    pub body: String,
    pub ttl: i32,
    pub r#type: Option<String>,
    pub closable: Option<bool>,
    pub persist: Option<bool>,
    pub wait_for_visibility: Option<bool>,
    pub sound: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexAccessToken {
    pub id: String,
    /// The time to live in seconds (2 weeks by default).
    pub ttl: Option<f64>,
    pub created: Option<DateTime<Utc>>,
    pub user_id: Option<f64>,
}

/// Daily Quote Fill Ratio Statistic.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexQuoteFillRatio {
    pub date: DateTime<Utc>,
    pub account: Option<f64>,
    pub quote_count: Option<f64>,
    pub dealt_count: Option<f64>,
    pub quotes_mavg7: Option<f64>,
    pub dealt_mavg7: Option<f64>,
    pub quote_fill_ratio_mavg7: Option<f64>,
}

/// Account Operations.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexUser {
    pub id: Option<i32>,
    pub owner_id: Option<i32>,
    pub firstname: Option<String>,
    pub lastname: Option<String>,
    pub username: String,
    pub email: String,
    pub phone: Option<String>,
    pub created: Option<DateTime<Utc>>,
    pub last_updated: Option<DateTime<Utc>>,
    pub preferences: BitmexUserPreferences,
    #[serde(rename = "TFAEnabled")]
    pub tfa_enabled: Option<String>,
    #[serde(rename = "affiliateID")]
    pub affiliate_id: Option<String>,
    pub pgp_pub_key: Option<String>,
    pub country: Option<String>,
    pub geoip_country: Option<String>,
    pub geoip_region: Option<String>,
    pub typ: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexMargin {
    pub account: i64,
    pub currency: Ustr,
    pub risk_limit: Option<i64>,
    pub prev_state: Option<String>,
    pub state: Option<String>,
    pub action: Option<String>,
    pub amount: Option<i64>,
    pub pending_credit: Option<i64>,
    pub pending_debit: Option<i64>,
    pub confirmed_debit: Option<i64>,
    pub prev_realised_pnl: Option<i64>,
    pub prev_unrealised_pnl: Option<i64>,
    pub gross_comm: Option<i64>,
    pub gross_open_cost: Option<i64>,
    pub gross_open_premium: Option<i64>,
    pub gross_exec_cost: Option<i64>,
    pub gross_mark_value: Option<i64>,
    pub risk_value: Option<i64>,
    pub taxable_margin: Option<i64>,
    pub init_margin: Option<i64>,
    pub maint_margin: Option<i64>,
    pub session_margin: Option<i64>,
    pub target_excess_margin: Option<i64>,
    pub var_margin: Option<i64>,
    pub realised_pnl: Option<i64>,
    pub unrealised_pnl: Option<i64>,
    pub indicative_tax: Option<i64>,
    pub unrealised_profit: Option<i64>,
    pub synthetic_margin: Option<i64>,
    pub wallet_balance: Option<i64>,
    pub margin_balance: Option<i64>,
    pub margin_balance_pcnt: Option<f64>,
    pub margin_leverage: Option<f64>,
    pub margin_used_pcnt: Option<f64>,
    pub excess_margin: Option<i64>,
    pub excess_margin_pcnt: Option<f64>,
    pub available_margin: Option<i64>,
    pub withdrawable_margin: Option<i64>,
    pub timestamp: Option<DateTime<Utc>>,
    pub gross_last_value: Option<i64>,
    pub commission: Option<f64>,
}

/// User communication SNS token.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexCommunicationToken {
    pub id: String,
    #[serde(rename = "userId")]
    pub user_id: i32,
    #[serde(rename = "deviceToken")]
    pub device_token: String,
    pub channel: String,
}

/// User Events for auditing.
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexUserEvent {
    pub id: Option<f64>,
    #[serde(rename = "type")]
    pub r#type: String,
    pub status: String,
    #[serde(rename = "userId")]
    pub user_id: f64,
    #[serde(rename = "createdById")]
    pub created_by_id: Option<f64>,
    pub ip: Option<String>,
    #[serde(rename = "geoipCountry")]
    pub geoip_country: Option<String>,
    #[serde(rename = "geoipRegion")]
    pub geoip_region: Option<String>,
    #[serde(rename = "geoipSubRegion")]
    pub geoip_sub_region: Option<String>,
    #[serde(rename = "eventMeta")]
    pub event_meta: Option<BitmexEventMetaEventMeta>,
    pub created: DateTime<Utc>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct BitmexEventMetaEventMeta(serde_json::Value);

#[derive(Clone, Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct BitmexUserPreferences {
    pub alert_on_liquidations: Option<bool>,
    pub animations_enabled: Option<bool>,
    pub announcements_last_seen: Option<DateTime<Utc>>,
    pub chat_channel_id: Option<f64>,
    pub color_theme: Option<String>,
    pub currency: Option<Ustr>,
    pub debug: Option<bool>,
    pub disable_emails: Option<Vec<String>>,
    pub disable_push: Option<Vec<String>>,
    pub hide_confirm_dialogs: Option<Vec<String>>,
    pub hide_connection_modal: Option<bool>,
    pub hide_from_leaderboard: Option<bool>,
    pub hide_name_from_leaderboard: Option<bool>,
    pub hide_notifications: Option<Vec<String>>,
    pub locale: Option<String>,
    pub msgs_seen: Option<Vec<String>>,
    pub order_book_binning: Option<BitmexOrderBookBinning>,
    pub order_book_type: Option<String>,
    pub order_clear_immediate: Option<bool>,
    pub order_controls_plus_minus: Option<bool>,
    pub show_locale_numbers: Option<bool>,
    pub sounds: Option<Vec<String>>,
    #[serde(rename = "strictIPCheck")]
    pub strict_ip_check: Option<bool>,
    pub strict_timeout: Option<bool>,
    pub ticker_group: Option<String>,
    pub ticker_pinned: Option<bool>,
    pub trade_layout: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Default)]
#[allow(dead_code)]
pub struct BitmexOrderBookBinning(serde_json::Value);

/// Represents the response from `GET /api/v1` (root endpoint).
#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BitmexApiInfo {
    /// API name.
    pub name: String,
    /// API version.
    pub version: String,
    /// Server timestamp in milliseconds.
    pub timestamp: u64,
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    #[case(json!(null), None)]
    #[case(json!(""), None)]
    #[case(json!("ParticipateDoNotInitiate"), Some(vec![BitmexExecInstruction::ParticipateDoNotInitiate]))]
    #[case(json!("ReduceOnly"), Some(vec![BitmexExecInstruction::ReduceOnly]))]
    #[case(json!("LastPrice,Close"), Some(vec![BitmexExecInstruction::LastPrice, BitmexExecInstruction::Close]))]
    #[case(
        json!("ParticipateDoNotInitiate,ReduceOnly"),
        Some(vec![BitmexExecInstruction::ParticipateDoNotInitiate, BitmexExecInstruction::ReduceOnly])
    )]
    #[case(
        json!("MarkPrice,IndexPrice,AllOrNone"),
        Some(vec![BitmexExecInstruction::MarkPrice, BitmexExecInstruction::IndexPrice, BitmexExecInstruction::AllOrNone])
    )]
    #[case(json!("Fixed"), Some(vec![BitmexExecInstruction::Fixed]))]
    fn test_deserialize_exec_instructions(
        #[case] input: serde_json::Value,
        #[case] expected: Option<Vec<BitmexExecInstruction>>,
    ) {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(default, deserialize_with = "deserialize_exec_instructions")]
            exec_inst: Option<Vec<BitmexExecInstruction>>,
        }

        let test_json = json!({
            "exec_inst": input
        });

        let result: TestStruct = serde_json::from_value(test_json).unwrap();
        assert_eq!(result.exec_inst, expected);
    }

    #[rstest]
    fn test_deserialize_exec_instructions_with_spaces() {
        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(default, deserialize_with = "deserialize_exec_instructions")]
            exec_inst: Option<Vec<BitmexExecInstruction>>,
        }

        let test_json = json!({
            "exec_inst": "LastPrice , Close , ReduceOnly"
        });

        let result: TestStruct = serde_json::from_value(test_json).unwrap();
        assert_eq!(
            result.exec_inst,
            Some(vec![
                BitmexExecInstruction::LastPrice,
                BitmexExecInstruction::Close,
                BitmexExecInstruction::ReduceOnly,
            ])
        );
    }

    #[rstest]
    fn test_deserialize_order_with_exec_instructions() {
        let order_json = json!({
            "account": 123456,
            "symbol": "XBTUSD",
            "orderID": "a1b2c3d4-e5f6-7890-abcd-ef1234567890",
            "side": "Buy",
            "ordType": "Limit",
            "timeInForce": "GoodTillCancel",
            "ordStatus": "New",
            "orderQty": 100,
            "cumQty": 0,
            "price": 50000.0,
            "execInst": "ParticipateDoNotInitiate,ReduceOnly",
            "transactTime": "2024-01-01T00:00:00.000Z",
            "timestamp": "2024-01-01T00:00:00.000Z"
        });

        let order: BitmexOrder = serde_json::from_value(order_json).unwrap();
        assert_eq!(
            order.exec_inst,
            Some(vec![
                BitmexExecInstruction::ParticipateDoNotInitiate,
                BitmexExecInstruction::ReduceOnly,
            ])
        );
    }
}
