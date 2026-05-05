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

//! Enumerations for the Interactive Brokers adapter.

mod contracts;
mod events;
mod market_data;
mod misc;
mod order;

pub use contracts::*;
pub use events::*;
pub use market_data::*;
pub use misc::*;
pub use order::*;

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_model::enums::{
        OptionKind, OrderSide, OrderStatus as NautilusOrderStatus, OrderType as NautilusOrderType,
        TimeInForce as NautilusTimeInForce,
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("BUY", IbAction::Buy, OrderSide::Buy, 1)]
    #[case("BOT", IbAction::Bought, OrderSide::Buy, 1)]
    #[case("SELL", IbAction::Sell, OrderSide::Sell, -1)]
    #[case("SLD", IbAction::Sold, OrderSide::Sell, -1)]
    #[case("SSHORT", IbAction::SellShort, OrderSide::Sell, -1)]
    #[case("SLONG", IbAction::SellLong, OrderSide::Sell, -1)]
    fn test_ib_action_parse(
        #[case] value: &str,
        #[case] expected_action: IbAction,
        #[case] expected_side: OrderSide,
        #[case] expected_multiplier: i32,
    ) {
        let action = IbAction::from_str(value).unwrap();
        assert_eq!(action, expected_action);
        assert_eq!(action.order_side(), expected_side);
        assert_eq!(action.signed_multiplier(), expected_multiplier);
        assert_eq!(action.to_string(), value);
        assert_eq!(
            IbAction::from(action.ibapi_action()).order_side(),
            expected_side
        );
    }

    #[rstest]
    #[case(
        "ApiPending",
        IbOrderStatus::ApiPending,
        NautilusOrderStatus::Submitted
    )]
    #[case(
        "PendingSubmit",
        IbOrderStatus::PendingSubmit,
        NautilusOrderStatus::Submitted
    )]
    #[case(
        "PreSubmitted",
        IbOrderStatus::PreSubmitted,
        NautilusOrderStatus::Submitted
    )]
    #[case("Submitted", IbOrderStatus::Submitted, NautilusOrderStatus::Accepted)]
    #[case(
        "PendingCancel",
        IbOrderStatus::PendingCancel,
        NautilusOrderStatus::PendingCancel
    )]
    #[case(
        "ApiCancelled",
        IbOrderStatus::ApiCancelled,
        NautilusOrderStatus::Canceled
    )]
    #[case("Cancelled", IbOrderStatus::Cancelled, NautilusOrderStatus::Canceled)]
    #[case("Filled", IbOrderStatus::Filled, NautilusOrderStatus::Filled)]
    #[case("Inactive", IbOrderStatus::Inactive, NautilusOrderStatus::Rejected)]
    fn test_ib_order_status_parse(
        #[case] value: &str,
        #[case] expected_status: IbOrderStatus,
        #[case] expected_nautilus_status: NautilusOrderStatus,
    ) {
        let status = IbOrderStatus::from_str(value).unwrap();
        assert_eq!(status, expected_status);
        assert_eq!(status.nautilus_status(), expected_nautilus_status);
        assert_eq!(status.to_string(), value);
    }

    #[rstest]
    #[case("MKT", IbOrderType::Market, NautilusOrderType::Market)]
    #[case("MOC", IbOrderType::MarketOnClose, NautilusOrderType::Market)]
    #[case("LMT", IbOrderType::Limit, NautilusOrderType::Limit)]
    #[case("LOC", IbOrderType::LimitOnClose, NautilusOrderType::Limit)]
    #[case("STP", IbOrderType::Stop, NautilusOrderType::StopMarket)]
    #[case("STP LMT", IbOrderType::StopLimit, NautilusOrderType::StopLimit)]
    #[case(
        "TRAIL",
        IbOrderType::TrailingStop,
        NautilusOrderType::TrailingStopMarket
    )]
    #[case(
        "TRAIL LIMIT",
        IbOrderType::TrailingStopLimit,
        NautilusOrderType::TrailingStopLimit
    )]
    #[case(
        "MIT",
        IbOrderType::MarketIfTouched,
        NautilusOrderType::MarketIfTouched
    )]
    #[case("LIT", IbOrderType::LimitIfTouched, NautilusOrderType::LimitIfTouched)]
    #[case("MTL", IbOrderType::MarketToLimit, NautilusOrderType::MarketToLimit)]
    fn test_ib_order_type_parse(
        #[case] value: &str,
        #[case] expected_order_type: IbOrderType,
        #[case] expected_nautilus_order_type: NautilusOrderType,
    ) {
        let order_type = IbOrderType::from_str(value).unwrap();
        assert_eq!(order_type, expected_order_type);
        assert_eq!(
            order_type.nautilus_order_type(),
            expected_nautilus_order_type
        );
        assert_eq!(order_type.to_string(), value);
    }

    #[rstest]
    #[case("PEGMID", IbOrderType::PeggedToMidpoint, "PEG MID")]
    #[case("PEGBENCH", IbOrderType::PeggedToBenchmark, "PEG BENCH")]
    #[case("PEGBEST", IbOrderType::PegBest, "PEG BEST")]
    fn test_ib_order_type_parse_accepts_ibapi_python_aliases(
        #[case] value: &str,
        #[case] expected_order_type: IbOrderType,
        #[case] expected_display: &str,
    ) {
        let order_type = IbOrderType::from_str(value).unwrap();
        assert_eq!(order_type, expected_order_type);
        assert_eq!(order_type.to_string(), expected_display);
    }

    #[rstest]
    fn test_ib_order_type_wire_roundtrip_preserves_nautilus_order_type() {
        let order_types = [
            IbOrderType::Market,
            IbOrderType::MarketOnClose,
            IbOrderType::Limit,
            IbOrderType::LimitOnClose,
            IbOrderType::Stop,
            IbOrderType::StopLimit,
            IbOrderType::TrailingStop,
            IbOrderType::TrailingStopLimit,
            IbOrderType::MarketIfTouched,
            IbOrderType::LimitIfTouched,
            IbOrderType::MarketToLimit,
            IbOrderType::MarketWithProtection,
            IbOrderType::StopWithProtection,
            IbOrderType::Midprice,
            IbOrderType::PeggedToMarket,
            IbOrderType::PeggedToStock,
            IbOrderType::PeggedToMidpoint,
            IbOrderType::PeggedToBenchmark,
            IbOrderType::PegBest,
            IbOrderType::Relative,
            IbOrderType::PassiveRelative,
            IbOrderType::Volatility,
            IbOrderType::BoxTop,
            IbOrderType::RelativeLimitCombo,
            IbOrderType::RelativeMarketCombo,
        ];

        for order_type in order_types {
            let parsed = IbOrderType::from_str(order_type.as_str()).unwrap();

            assert_eq!(
                parsed.nautilus_order_type(),
                order_type.nautilus_order_type(),
                "{} round-tripped through {:?}",
                order_type.as_str(),
                parsed,
            );
        }
    }

    #[rstest]
    #[case("DAY", IbTimeInForce::Day, NautilusTimeInForce::Day)]
    #[case("GTC", IbTimeInForce::GoodTilCanceled, NautilusTimeInForce::Gtc)]
    #[case("IOC", IbTimeInForce::ImmediateOrCancel, NautilusTimeInForce::Ioc)]
    #[case("GTD", IbTimeInForce::GoodTilDate, NautilusTimeInForce::Gtd)]
    #[case("OPG", IbTimeInForce::OnOpen, NautilusTimeInForce::AtTheOpen)]
    #[case("FOK", IbTimeInForce::FillOrKill, NautilusTimeInForce::Fok)]
    #[case("DTC", IbTimeInForce::DayTilCanceled, NautilusTimeInForce::Day)]
    #[case("AUC", IbTimeInForce::Auction, NautilusTimeInForce::Day)]
    fn test_ib_time_in_force_parse(
        #[case] value: &str,
        #[case] expected_time_in_force: IbTimeInForce,
        #[case] expected_nautilus_time_in_force: NautilusTimeInForce,
    ) {
        let time_in_force = IbTimeInForce::from_str(value).unwrap();
        assert_eq!(time_in_force, expected_time_in_force);
        assert_eq!(
            time_in_force.nautilus_time_in_force(),
            expected_nautilus_time_in_force
        );
        assert_eq!(time_in_force.to_string(), value);
        assert_eq!(
            IbTimeInForce::from(time_in_force.ibapi_time_in_force()),
            expected_time_in_force
        );
    }

    #[rstest]
    #[case("STK", IbSecurityType::Stock)]
    #[case("OPT", IbSecurityType::Option)]
    #[case("FUT", IbSecurityType::Future)]
    #[case("CONTFUT", IbSecurityType::ContinuousFuture)]
    #[case("IND", IbSecurityType::Index)]
    #[case("FOP", IbSecurityType::FuturesOption)]
    #[case("CASH", IbSecurityType::ForexPair)]
    #[case("BAG", IbSecurityType::Spread)]
    #[case("WAR", IbSecurityType::Warrant)]
    #[case("BOND", IbSecurityType::Bond)]
    #[case("CMDTY", IbSecurityType::Commodity)]
    #[case("NEWS", IbSecurityType::News)]
    #[case("FUND", IbSecurityType::MutualFund)]
    #[case("CRYPTO", IbSecurityType::Crypto)]
    #[case("CFD", IbSecurityType::Cfd)]
    fn test_ib_security_type_parse(
        #[case] value: &str,
        #[case] expected_security_type: IbSecurityType,
    ) {
        let security_type = IbSecurityType::from_str(value).unwrap();
        assert_eq!(security_type, expected_security_type);
        assert_eq!(security_type.to_string(), value);
        assert_eq!(
            IbSecurityType::try_from(&security_type.ibapi_security_type()).unwrap(),
            expected_security_type
        );
    }

    #[rstest]
    #[case("C", IbOptionRight::Call, OptionKind::Call)]
    #[case("P", IbOptionRight::Put, OptionKind::Put)]
    #[case("CALL", IbOptionRight::Call, OptionKind::Call)]
    #[case("PUT", IbOptionRight::Put, OptionKind::Put)]
    fn test_ib_option_right_parse(
        #[case] value: &str,
        #[case] expected_right: IbOptionRight,
        #[case] expected_option_kind: OptionKind,
    ) {
        let right = IbOptionRight::from_str(value).unwrap();
        assert_eq!(right, expected_right);
        assert_eq!(right.option_kind(), expected_option_kind);
    }

    #[rstest]
    #[case("TRADES", IbHistoricalTickType::Trades)]
    #[case("BID_ASK", IbHistoricalTickType::BidAsk)]
    fn test_ib_historical_tick_type_parse(
        #[case] value: &str,
        #[case] expected_tick_type: IbHistoricalTickType,
    ) {
        let tick_type = IbHistoricalTickType::from_str(value).unwrap();
        assert_eq!(tick_type, expected_tick_type);
        assert_eq!(tick_type.to_string(), value);
    }

    #[rstest]
    #[case(true, IbTradingHours::Regular)]
    #[case(false, IbTradingHours::Extended)]
    fn test_ib_trading_hours_parse(
        #[case] use_rth: bool,
        #[case] expected_trading_hours: IbTradingHours,
    ) {
        let trading_hours = IbTradingHours::from(use_rth);
        assert_eq!(trading_hours, expected_trading_hours);
        assert_eq!(trading_hours.use_rth(), use_rth);
        assert_eq!(
            trading_hours.ibapi_trading_hours().use_rth(),
            expected_trading_hours.use_rth()
        );
    }

    #[rstest]
    #[case(IbHistoricalBarSize::Sec, "1 secs")]
    #[case(IbHistoricalBarSize::Min5, "5 mins")]
    #[case(IbHistoricalBarSize::Hour2, "2 hours")]
    #[case(IbHistoricalBarSize::Day, "1 day")]
    fn test_ib_historical_bar_size_display(
        #[case] bar_size: IbHistoricalBarSize,
        #[case] expected_display: &str,
    ) {
        assert_eq!(bar_size.to_string(), expected_display);
    }

    #[rstest]
    #[case(IbHistoricalWhatToShow::Trades, "TRADES")]
    #[case(IbHistoricalWhatToShow::Midpoint, "MIDPOINT")]
    #[case(IbHistoricalWhatToShow::BidAsk, "BID_ASK")]
    #[case(IbHistoricalWhatToShow::AdjustedLast, "ADJUSTED_LAST")]
    fn test_ib_historical_what_to_show_display(
        #[case] what_to_show: IbHistoricalWhatToShow,
        #[case] expected_display: &str,
    ) {
        assert_eq!(what_to_show.as_str(), expected_display);
        assert_eq!(what_to_show.to_string(), expected_display);
    }

    #[rstest]
    #[case(IbRealtimeBarSize::Sec5, "5 secs")]
    fn test_ib_realtime_bar_size_display(
        #[case] bar_size: IbRealtimeBarSize,
        #[case] expected_display: &str,
    ) {
        assert_eq!(bar_size.to_string(), expected_display);
    }

    #[rstest]
    #[case(IbRealtimeWhatToShow::Trades, "TRADES")]
    #[case(IbRealtimeWhatToShow::Midpoint, "MIDPOINT")]
    #[case(IbRealtimeWhatToShow::Bid, "BID")]
    #[case(IbRealtimeWhatToShow::Ask, "ASK")]
    fn test_ib_realtime_what_to_show_display(
        #[case] what_to_show: IbRealtimeWhatToShow,
        #[case] expected_display: &str,
    ) {
        assert_eq!(what_to_show.as_str(), expected_display);
        assert_eq!(what_to_show.to_string(), expected_display);
    }

    #[rstest]
    #[case("price", IbConditionKind::Price)]
    #[case("time", IbConditionKind::Time)]
    #[case("margin", IbConditionKind::Margin)]
    #[case("execution", IbConditionKind::Execution)]
    #[case("volume", IbConditionKind::Volume)]
    #[case("percent_change", IbConditionKind::PercentChange)]
    fn test_ib_condition_kind_parse(#[case] value: &str, #[case] expected_kind: IbConditionKind) {
        let kind = IbConditionKind::from_str(value).unwrap();
        assert_eq!(kind, expected_kind);
        assert_eq!(kind.to_string(), value);
    }

    #[rstest]
    #[case("and", IbConditionConjunction::And, true)]
    #[case("or", IbConditionConjunction::Or, false)]
    #[case("a", IbConditionConjunction::And, true)]
    #[case("o", IbConditionConjunction::Or, false)]
    fn test_ib_condition_conjunction_parse(
        #[case] value: &str,
        #[case] expected_conjunction: IbConditionConjunction,
        #[case] expected_is_conjunction: bool,
    ) {
        let conjunction = IbConditionConjunction::from_str(value).unwrap();
        assert_eq!(conjunction, expected_conjunction);
        assert_eq!(conjunction.is_conjunction(), expected_is_conjunction);
    }

    #[rstest]
    #[case(0, IbTriggerMethod::Default)]
    #[case(1, IbTriggerMethod::DoubleBidAsk)]
    #[case(2, IbTriggerMethod::Last)]
    #[case(3, IbTriggerMethod::DoubleLast)]
    #[case(4, IbTriggerMethod::BidAsk)]
    #[case(7, IbTriggerMethod::LastOrBidAsk)]
    #[case(8, IbTriggerMethod::Midpoint)]
    fn test_ib_trigger_method_parse(
        #[case] value: i32,
        #[case] expected_trigger_method: IbTriggerMethod,
    ) {
        let trigger_method = IbTriggerMethod::from(value);
        assert_eq!(trigger_method, expected_trigger_method);
        assert_eq!(trigger_method.as_i32(), value);
        assert_eq!(
            IbTriggerMethod::from(trigger_method.ibapi_trigger_method()),
            expected_trigger_method
        );
    }

    #[rstest]
    #[case(0, IbOcaType::None)]
    #[case(1, IbOcaType::CancelWithBlock)]
    #[case(2, IbOcaType::ReduceWithBlock)]
    #[case(3, IbOcaType::ReduceWithoutBlock)]
    fn test_ib_oca_type_parse(#[case] value: i32, #[case] expected_oca_type: IbOcaType) {
        let oca_type = IbOcaType::from(value);
        assert_eq!(oca_type, expected_oca_type);
        assert_eq!(oca_type.as_i32(), value);
        assert_eq!(
            IbOcaType::from(oca_type.ibapi_oca_type()),
            expected_oca_type
        );
    }

    #[rstest]
    #[case(0, IbComboLegOpenClose::Same)]
    #[case(1, IbComboLegOpenClose::Open)]
    #[case(2, IbComboLegOpenClose::Close)]
    #[case(3, IbComboLegOpenClose::Unknown)]
    fn test_ib_combo_leg_open_close_parse(
        #[case] value: i32,
        #[case] expected_open_close: IbComboLegOpenClose,
    ) {
        assert_eq!(expected_open_close.as_i32(), value);
        assert_eq!(
            IbComboLegOpenClose::from(expected_open_close.ibapi_combo_leg_open_close()),
            expected_open_close
        );
    }

    #[rstest]
    #[case(0, IbLiquidity::None)]
    #[case(1, IbLiquidity::AddedLiquidity)]
    #[case(2, IbLiquidity::RemovedLiquidity)]
    #[case(3, IbLiquidity::LiquidityRoutedOut)]
    fn test_ib_liquidity_parse(#[case] value: i32, #[case] expected_liquidity: IbLiquidity) {
        let liquidity = IbLiquidity::from(value);
        assert_eq!(liquidity, expected_liquidity);
        assert_eq!(liquidity.as_i32(), value);
        assert_eq!(
            IbLiquidity::from(liquidity.ibapi_liquidity()),
            expected_liquidity
        );
    }
}
