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

//! Betfair REST/JSON-RPC API model types.
//!
//! These types cover the Betting API, Accounts API, Identity API, and
//! Navigation API. All use camelCase JSON field naming.
//!
//! # References
//!
//! <https://docs.developer.betfair.com/>

use ahash::AHashMap;
use nautilus_core::serialization::{deserialize_decimal, deserialize_optional_decimal};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::common::{
    enums::{
        BetDelayModel, BetStatus, BetTargetType, BetfairOrderStatus, BetfairOrderType, BetfairSide,
        BetfairTimeInForce, CertLoginStatus, ExecutionReportErrorCode, ExecutionReportStatus,
        GroupBy, InstructionReportErrorCode, InstructionReportStatus, MarketBettingType,
        MarketProjection, MarketSort, OrderBy, OrderProjection, PersistenceType, PriceLadderType,
        SortDir,
    },
    types::{
        BetId, CompetitionId, CustomerOrderRef, CustomerStrategyRef, EventId, EventTypeId,
        Handicap, MarketId, SelectionId, deserialize_optional_string_lenient,
        deserialize_optional_u32_lenient,
    },
};

/// Login status.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum LoginStatus {
    Success,
    LimitedAccess,
    LoginRestricted,
    Fail,
}

/// Login response from the interactive Identity SSO API.
#[derive(Debug, Clone, Deserialize)]
pub struct LoginResponse {
    pub token: String,
    pub product: String,
    pub status: LoginStatus,
    pub error: Option<String>,
}

/// Login response from the certificate-based SSO API (`certlogin`).
///
/// Uses different field names from the interactive login endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CertLoginResponse {
    pub session_token: Option<String>,
    pub login_status: CertLoginStatus,
}

/// Account details response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountDetailsResponse {
    pub currency_code: Option<Ustr>,
    pub first_name: Option<String>,
    pub last_name: Option<String>,
    pub locale_code: Option<Ustr>,
    pub region: Option<Ustr>,
    pub timezone: Option<Ustr>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub discount_rate: Option<Decimal>,
    pub points_balance: Option<i64>,
    pub country_code: Option<Ustr>,
}

/// Account funds response.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountFundsResponse {
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub available_to_bet_balance: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub exposure: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub retained_commission: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub exposure_limit: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub discount_rate: Option<Decimal>,
    pub points_balance: Option<i64>,
    pub wallet: Option<Ustr>,
}

/// Time range filter.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TimeRange {
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Price-size pair.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceSize {
    #[serde(deserialize_with = "deserialize_decimal")]
    pub price: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub size: Decimal,
}

/// Market version for price protection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketVersion {
    pub version: Option<i64>,
}

/// Event type (e.g. "Soccer", "Horse Racing").
#[derive(Debug, Clone, Deserialize)]
pub struct EventType {
    #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
    pub id: Option<EventTypeId>,
    pub name: Option<Ustr>,
}

/// Event (e.g. a specific football match).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
    pub id: Option<EventId>,
    pub name: Option<String>,
    pub country_code: Option<Ustr>,
    pub timezone: Option<Ustr>,
    pub venue: Option<Ustr>,
    pub open_date: Option<String>,
}

/// Competition (e.g. "English Premier League").
#[derive(Debug, Clone, Deserialize)]
pub struct Competition {
    #[serde(default, deserialize_with = "deserialize_optional_string_lenient")]
    pub id: Option<CompetitionId>,
    pub name: Option<String>,
}

/// Runner identifier.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerId {
    pub market_id: MarketId,
    pub selection_id: SelectionId,
    pub handicap: Option<Handicap>,
}

/// Market catalogue entry returned by `listMarketCatalogue`.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketCatalogue {
    pub market_id: MarketId,
    pub market_name: String,
    pub market_start_time: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub total_matched: Option<Decimal>,
    pub event_type: Option<EventType>,
    pub competition: Option<Competition>,
    pub description: Option<MarketDescription>,
    pub event: Option<Event>,
    pub runners: Option<Vec<RunnerCatalog>>,
}

/// Detailed market description.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketDescription {
    pub betting_type: MarketBettingType,
    pub bsp_market: bool,
    pub clarifications: Option<String>,
    pub discount_allowed: bool,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub each_way_divisor: Option<Decimal>,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub market_base_rate: Decimal,
    pub market_time: String,
    pub market_type: Ustr,
    pub persistence_enabled: bool,
    pub race_type: Option<Ustr>,
    pub regulator: Ustr,
    pub rules: Option<String>,
    pub rules_has_date: Option<bool>,
    pub settle_time: Option<String>,
    pub suspend_time: String,
    pub turn_in_play_enabled: bool,
    pub wallet: Option<Ustr>,
    pub bet_delay_models: Option<Vec<BetDelayModel>>,
    pub line_range_info: Option<LineRangeInfo>,
    pub price_ladder_description: Option<PriceLadderDescription>,
}

/// Price ladder description within a market.
#[derive(Debug, Clone, Deserialize)]
pub struct PriceLadderDescription {
    #[serde(rename = "type")]
    pub ladder_type: Option<PriceLadderType>,
}

/// Line range info for line markets.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineRangeInfo {
    #[serde(deserialize_with = "deserialize_decimal")]
    pub max_unit_value: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub min_unit_value: Decimal,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub interval: Decimal,
}

/// Runner catalog entry (static runner information).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RunnerCatalog {
    pub selection_id: SelectionId,
    pub runner_name: String,
    pub handicap: Handicap,
    pub sort_priority: Option<u32>,
    /// Free-form metadata keyed by SCREAMING_SNAKE_CASE field names.
    ///
    /// The Betfair API defines this as `Map<String, String>`, but in practice
    /// values may be JSON numbers (e.g. AGE, CLOTH_NUMBER, STALL_DRAW). Keys
    /// and available fields vary by sport and market type.
    pub metadata: Option<AHashMap<String, serde_json::Value>>,
}

/// Market filter for REST API queries (e.g. `listMarketCatalogue`).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MarketFilter {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bsp_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub competition_ids: Option<Vec<CompetitionId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ids: Option<Vec<EventId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type_ids: Option<Vec<EventTypeId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub in_play_only: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_betting_types: Option<Vec<MarketBettingType>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_countries: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ids: Option<Vec<MarketId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_start_time: Option<TimeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_type_codes: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub race_types: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text_query: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub turn_in_play_enabled: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub venues: Option<Vec<Ustr>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub with_orders: Option<Vec<String>>,
}

/// Limit order parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LimitOrder {
    pub size: Decimal,
    pub price: Decimal,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persistence_type: Option<PersistenceType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub time_in_force: Option<BetfairTimeInForce>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_fill_size: Option<Decimal>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bet_target_type: Option<BetTargetType>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bet_target_size: Option<Decimal>,
}

/// Limit-on-close order parameters (for BSP markets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LimitOnCloseOrder {
    pub liability: Decimal,
    pub price: Decimal,
}

/// Market-on-close order parameters (for BSP markets).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketOnCloseOrder {
    pub liability: Decimal,
}

/// Instruction to place a new order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceInstruction {
    pub order_type: BetfairOrderType,
    pub selection_id: SelectionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub handicap: Option<Handicap>,
    pub side: BetfairSide,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_order: Option<LimitOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub limit_on_close_order: Option<LimitOnCloseOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_on_close_order: Option<MarketOnCloseOrder>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_order_ref: Option<String>,
}

/// Instruction to cancel an existing order.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelInstruction {
    pub bet_id: BetId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size_reduction: Option<Decimal>,
}

/// Instruction to replace an existing order (cancel + place at new price).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceInstruction {
    pub bet_id: BetId,
    pub new_price: Decimal,
}

/// Parameters for a `placeOrders` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceOrdersParams {
    pub market_id: MarketId,
    pub instructions: Vec<PlaceInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_version: Option<MarketVersion>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_strategy_ref: Option<CustomerStrategyRef>,
}

/// Parameters for a `cancelOrders` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelOrdersParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_id: Option<MarketId>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instructions: Option<Vec<CancelInstruction>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_ref: Option<String>,
}

/// Parameters for a `replaceOrders` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceOrdersParams {
    pub market_id: MarketId,
    pub instructions: Vec<ReplaceInstruction>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_ref: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_version: Option<MarketVersion>,
}

/// Parameters for a `listMarketCatalogue` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListMarketCatalogueParams {
    pub filter: MarketFilter,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_projection: Option<Vec<MarketProjection>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort: Option<MarketSort>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_results: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
}

/// Parameters for a `listCurrentOrders` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListCurrentOrdersParams {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bet_ids: Option<Vec<BetId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ids: Option<Vec<MarketId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_projection: Option<OrderProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_order_refs: Option<Vec<CustomerOrderRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_strategy_refs: Option<Vec<CustomerStrategyRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date_range: Option<TimeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub order_by: Option<OrderBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sort_dir: Option<SortDir>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_record: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<u32>,
}

/// Parameters for a `listClearedOrders` request.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListClearedOrdersParams {
    pub bet_status: BetStatus,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_type_ids: Option<Vec<EventTypeId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event_ids: Option<Vec<EventId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_ids: Option<Vec<MarketId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub runner_ids: Option<Vec<RunnerId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bet_ids: Option<Vec<BetId>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_order_refs: Option<Vec<CustomerOrderRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub customer_strategy_refs: Option<Vec<CustomerStrategyRef>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub side: Option<BetfairSide>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub settled_date_range: Option<TimeRange>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group_by: Option<GroupBy>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub include_item_description: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub locale: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from_record: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub record_count: Option<u32>,
}

/// Response to a `placeOrders` request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceExecutionReport {
    pub customer_ref: Option<String>,
    pub status: ExecutionReportStatus,
    pub error_code: Option<ExecutionReportErrorCode>,
    pub error_message: Option<String>,
    pub market_id: Option<MarketId>,
    pub instruction_reports: Option<Vec<PlaceInstructionReport>>,
}

/// Individual instruction report for a place operation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlaceInstructionReport {
    pub status: InstructionReportStatus,
    pub error_code: Option<InstructionReportErrorCode>,
    pub error_message: Option<String>,
    pub order_status: Option<BetfairOrderStatus>,
    pub instruction: Option<PlaceInstruction>,
    pub bet_id: Option<BetId>,
    pub placed_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub average_price_matched: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_matched: Option<Decimal>,
}

/// Response to a `cancelOrders` request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelExecutionReport {
    pub customer_ref: Option<String>,
    pub status: ExecutionReportStatus,
    pub error_code: Option<ExecutionReportErrorCode>,
    pub error_message: Option<String>,
    pub market_id: Option<MarketId>,
    pub instruction_reports: Option<Vec<CancelInstructionReport>>,
}

/// Individual instruction report for a cancel operation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CancelInstructionReport {
    pub status: InstructionReportStatus,
    pub error_code: Option<InstructionReportErrorCode>,
    pub error_message: Option<String>,
    pub instruction: Option<CancelInstruction>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_cancelled: Option<Decimal>,
    pub cancelled_date: Option<String>,
}

/// Response to a `replaceOrders` request.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceExecutionReport {
    pub customer_ref: Option<String>,
    pub status: ExecutionReportStatus,
    pub error_code: Option<ExecutionReportErrorCode>,
    pub error_message: Option<String>,
    pub market_id: Option<MarketId>,
    pub instruction_reports: Option<Vec<ReplaceInstructionReport>>,
}

/// Individual instruction report for a replace operation.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReplaceInstructionReport {
    pub status: InstructionReportStatus,
    pub error_code: Option<InstructionReportErrorCode>,
    pub error_message: Option<String>,
    pub cancel_instruction_report: Option<CancelInstructionReport>,
    pub place_instruction_report: Option<PlaceInstructionReport>,
}

/// Current (active) order summary.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentOrderSummary {
    pub bet_id: BetId,
    pub market_id: MarketId,
    pub selection_id: SelectionId,
    pub handicap: Handicap,
    pub price_size: PriceSize,
    #[serde(deserialize_with = "deserialize_decimal")]
    pub bsp_liability: Decimal,
    pub side: BetfairSide,
    pub status: BetfairOrderStatus,
    pub persistence_type: PersistenceType,
    pub order_type: BetfairOrderType,
    pub placed_date: String,
    pub matched_date: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub average_price_matched: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_matched: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_remaining: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_lapsed: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_cancelled: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_voided: Option<Decimal>,
    pub regulator_auth_code: Option<String>,
    pub regulator_code: Option<String>,
    pub customer_order_ref: Option<CustomerOrderRef>,
    pub customer_strategy_ref: Option<CustomerStrategyRef>,
}

/// Report containing current order summaries.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CurrentOrderSummaryReport {
    pub current_orders: Vec<CurrentOrderSummary>,
    pub more_available: bool,
}

/// Item description for cleared orders (present when `includeItemDescription=true`).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemDescription {
    pub event_type_desc: Option<String>,
    pub event_desc: Option<String>,
    pub market_desc: Option<String>,
    pub market_type: Option<Ustr>,
    pub market_start_time: Option<String>,
    pub runner_desc: Option<String>,
    pub number_of_winners: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub each_way_divisor: Option<Decimal>,
}

/// Cleared (settled/voided/lapsed/cancelled) order summary.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearedOrderSummary {
    pub event_type_id: Option<EventTypeId>,
    pub event_id: Option<EventId>,
    pub market_id: Option<MarketId>,
    pub selection_id: Option<SelectionId>,
    pub handicap: Option<Handicap>,
    pub bet_id: Option<BetId>,
    pub placed_date: Option<String>,
    pub persistence_type: Option<PersistenceType>,
    pub order_type: Option<BetfairOrderType>,
    pub side: Option<BetfairSide>,
    pub item_description: Option<ItemDescription>,
    pub bet_outcome: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub price_requested: Option<Decimal>,
    pub settled_date: Option<String>,
    pub last_matched_date: Option<String>,
    pub bet_count: Option<u32>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub commission: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub price_matched: Option<Decimal>,
    pub price_reduced: Option<bool>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_settled: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub profit: Option<Decimal>,
    #[serde(default, deserialize_with = "deserialize_optional_decimal")]
    pub size_cancelled: Option<Decimal>,
    pub customer_order_ref: Option<CustomerOrderRef>,
    pub customer_strategy_ref: Option<CustomerStrategyRef>,
}

/// Report containing cleared order summaries.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClearedOrderSummaryReport {
    pub cleared_orders: Vec<ClearedOrderSummary>,
    pub more_available: bool,
}

/// Market entry in the navigation tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationMarket {
    pub name: Option<String>,
    pub id: Option<MarketId>,
    pub exchange_id: Option<String>,
    pub market_type: Option<Ustr>,
    pub market_start_time: Option<String>,
    #[serde(default, deserialize_with = "deserialize_optional_u32_lenient")]
    pub number_of_winners: Option<u32>,
}

/// Race entry in the navigation tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationRace {
    pub name: Option<String>,
    pub id: Option<String>,
    pub venue: Option<Ustr>,
    pub start_time: Option<String>,
    pub race_number: Option<String>,
    pub country_code: Option<Ustr>,
    pub children: Option<Vec<NavigationChild>>,
}

/// Event entry in the navigation tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEvent {
    pub name: Option<String>,
    pub id: Option<EventId>,
    pub country_code: Option<Ustr>,
    pub children: Option<Vec<NavigationChild>>,
}

/// Group entry in the navigation tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationGroup {
    pub name: Option<String>,
    pub id: Option<String>,
    pub children: Option<Vec<NavigationChild>>,
}

/// Event type (top-level category) in the navigation tree.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NavigationEventType {
    pub name: Option<Ustr>,
    pub id: Option<EventTypeId>,
    pub children: Option<Vec<NavigationChild>>,
}

/// Child node in the navigation tree (polymorphic).
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum NavigationChild {
    #[serde(rename = "EVENT_TYPE")]
    EventType(NavigationEventType),
    #[serde(rename = "GROUP")]
    Group(NavigationGroup),
    #[serde(rename = "EVENT")]
    Event(NavigationEvent),
    #[serde(rename = "RACE")]
    Race(NavigationRace),
    #[serde(rename = "MARKET")]
    Market(NavigationMarket),
}

/// Root navigation response.
#[derive(Debug, Clone, Deserialize)]
pub struct Navigation {
    pub children: Option<Vec<NavigationChild>>,
}

/// Flattened (denormalized) view of a market from the navigation tree.
#[derive(Debug, Clone)]
pub struct FlattenedMarket {
    pub event_type_id: Option<String>,
    pub event_type_name: Option<Ustr>,
    pub event_id: Option<String>,
    pub event_name: Option<String>,
    pub event_country_code: Option<Ustr>,
    pub market_id: Option<MarketId>,
    pub market_name: Option<String>,
    pub market_type: Option<Ustr>,
    pub market_start_time: Option<String>,
    pub number_of_winners: Option<u32>,
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::testing::{load_test_json, parse_jsonrpc};

    #[rstest]
    fn test_cert_login_response() {
        let data = load_test_json("rest/cert_login.json");
        let resp: CertLoginResponse = serde_json::from_str(&data).unwrap();
        assert_eq!(resp.login_status, CertLoginStatus::Success);
        assert!(resp.session_token.is_some());
    }

    #[rstest]
    fn test_cert_login_error_response() {
        let json = r#"{"loginStatus":"CERT_AUTH_REQUIRED"}"#;
        let resp: CertLoginResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.login_status, CertLoginStatus::CertAuthRequired);
        assert!(resp.session_token.is_none());
    }

    #[rstest]
    fn test_cert_login_unknown_status_deserializes_to_other() {
        let json = r#"{"loginStatus":"SOME_FUTURE_CODE"}"#;
        let resp: CertLoginResponse = serde_json::from_str(json).unwrap();
        assert_eq!(resp.login_status, CertLoginStatus::Other);
    }

    #[rstest]
    fn test_interactive_login_response() {
        let data = load_test_json("rest/login_success.json");
        let resp: LoginResponse = serde_json::from_str(&data).unwrap();
        assert_eq!(resp.status, LoginStatus::Success);
    }

    #[rstest]
    fn test_interactive_login_failure() {
        let data = load_test_json("rest/login_failure.json");
        let resp: LoginResponse = serde_json::from_str(&data).unwrap();
        assert_eq!(resp.status, LoginStatus::Fail);
    }

    #[rstest]
    fn test_list_market_catalogue_with_runner_metadata() {
        let data = load_test_json("rest/list_market_catalogue.json");
        let catalogue: MarketCatalogue = serde_json::from_str(&data).unwrap();
        let runners = catalogue.runners.expect("runners present");
        let meta = runners[0].metadata.as_ref().expect("metadata present");
        assert!(meta.contains_key("AGE"));
        assert!(meta.contains_key("CLOTH_NUMBER"));
        assert!(meta.contains_key("STALL_DRAW"));
    }

    #[rstest]
    fn test_navigation_with_empty_number_of_winners() {
        let data = load_test_json("rest/navigation_list_navigation.json");
        let nav: Navigation = serde_json::from_str(&data).unwrap();
        assert!(nav.children.is_some());
    }

    fn find_race_with_children(children: &[NavigationChild]) -> bool {
        for child in children {
            match child {
                NavigationChild::Race(race) => {
                    if let Some(kids) = &race.children
                        && !kids.is_empty()
                    {
                        return true;
                    }
                }
                NavigationChild::EventType(et) => {
                    if let Some(kids) = &et.children
                        && find_race_with_children(kids)
                    {
                        return true;
                    }
                }
                NavigationChild::Group(g) => {
                    if let Some(kids) = &g.children
                        && find_race_with_children(kids)
                    {
                        return true;
                    }
                }
                NavigationChild::Event(e) => {
                    if let Some(kids) = &e.children
                        && find_race_with_children(kids)
                    {
                        return true;
                    }
                }
                NavigationChild::Market(_) => {}
            }
        }
        false
    }

    #[rstest]
    fn test_navigation_race_has_children() {
        let data = load_test_json("rest/navigation_list_navigation.json");
        let nav: Navigation = serde_json::from_str(&data).unwrap();
        let children = nav.children.as_ref().unwrap();
        assert!(
            find_race_with_children(children),
            "should find at least one RACE node with MARKET children"
        );
    }

    #[rstest]
    fn test_account_details() {
        let data = load_test_json("rest/account_details.json");
        let _resp: AccountDetailsResponse = serde_json::from_str(&data).unwrap();
    }

    #[rstest]
    #[case("rest/account_funds_no_exposure.json")]
    #[case("rest/account_funds_with_exposure.json")]
    fn test_account_funds(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let _resp: AccountFundsResponse =
            serde_json::from_str(&data).unwrap_or_else(|e| panic!("{fixture}: {e}"));
    }

    #[rstest]
    #[case("rest/betting_place_order_success.json")]
    #[case("rest/betting_place_order_error.json")]
    #[case("rest/betting_place_order_batch_success.json")]
    #[case("rest/betting_place_order_batch_partial_failure.json")]
    fn test_place_order_responses(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let _resp: PlaceExecutionReport = parse_jsonrpc(&data);
    }

    #[rstest]
    fn test_place_order_response_parses_instruction_error_message() {
        let data = r#"
        {
          "jsonrpc": "2.0",
          "result": {
            "status": "FAILURE",
            "instructionReports": [
              {
                "status": "FAILURE",
                "errorCode": "ERROR_IN_ORDER",
                "errorMessage": "Detailed Betfair validation message"
              }
            ]
          }
        }
        "#;

        let resp: PlaceExecutionReport = parse_jsonrpc(data);
        let instruction_report = resp
            .instruction_reports
            .as_ref()
            .and_then(|reports| reports.first())
            .expect("instruction report");

        assert_eq!(
            instruction_report.error_message.as_deref(),
            Some("Detailed Betfair validation message"),
        );
    }

    #[rstest]
    #[case("rest/betting_cancel_orders_success.json")]
    #[case("rest/betting_cancel_orders_error.json")]
    #[case("rest/betting_cancel_orders_batch_success.json")]
    #[case("rest/betting_cancel_orders_batch_partial_failure.json")]
    fn test_cancel_order_responses(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let _resp: CancelExecutionReport = parse_jsonrpc(&data);
    }

    #[rstest]
    fn test_replace_order_responses() {
        // betting_replace_orders_success_multi.json contains a streaming OCM,
        // not a REST ReplaceExecutionReport, so it is excluded
        let data = load_test_json("rest/betting_replace_orders_success.json");
        let _resp: ReplaceExecutionReport = parse_jsonrpc(&data);
    }

    #[rstest]
    #[case("rest/list_current_orders_empty.json")]
    #[case("rest/list_current_orders_single.json")]
    #[case("rest/list_current_orders_executable.json")]
    #[case("rest/list_current_orders_execution_complete.json")]
    #[case("rest/list_current_orders_on_close_execution_complete.json")]
    fn test_current_orders(#[case] fixture: &str) {
        let data = load_test_json(fixture);
        let _resp: CurrentOrderSummaryReport = parse_jsonrpc(&data);
    }

    #[rstest]
    fn test_cleared_orders() {
        let data = load_test_json("rest/list_cleared_orders.json");
        let _resp: ClearedOrderSummaryReport = parse_jsonrpc(&data);
    }

    #[rstest]
    fn test_betting_market_catalogue() {
        let data = load_test_json("rest/betting_list_market_catalogue.json");
        let catalogues: Vec<MarketCatalogue> = serde_json::from_str(&data).unwrap();
        assert!(!catalogues.is_empty());
    }
}
