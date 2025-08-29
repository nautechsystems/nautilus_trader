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

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use ustr::Ustr;

    use crate::{
        common::{
            enums::{OKXExecType, OKXInstrumentType, OKXMarginMode, OKXPositionSide, OKXSide},
            testing::load_test_json,
        },
        http::{
            client::OKXResponse,
            models::{
                OKXAccount, OKXBalanceDetail, OKXCandlestick, OKXIndexTicker, OKXMarkPrice,
                OKXOrderHistory, OKXPlaceOrderResponse, OKXPosition, OKXPositionHistory,
                OKXPositionTier, OKXTrade, OKXTransactionDetail,
            },
        },
    };

    #[rstest]
    fn test_parse_trades() {
        let json_data = load_test_json("http_get_trades.json");
        let parsed: OKXResponse<OKXTrade> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 2);

        // Inspect first record
        let trade0 = &parsed.data[0];
        assert_eq!(trade0.inst_id, "BTC-USDT");
        assert_eq!(trade0.px, "102537.9");
        assert_eq!(trade0.sz, "0.00013669");
        assert_eq!(trade0.side, OKXSide::Sell);
        assert_eq!(trade0.trade_id, "734864333");
        assert_eq!(trade0.ts, 1747087163557);

        // Inspect second record
        let trade1 = &parsed.data[1];
        assert_eq!(trade1.inst_id, "BTC-USDT");
        assert_eq!(trade1.px, "102537.9");
        assert_eq!(trade1.sz, "0.0000125");
        assert_eq!(trade1.side, OKXSide::Buy);
        assert_eq!(trade1.trade_id, "734864332");
        assert_eq!(trade1.ts, 1747087161666);
    }

    #[rstest]
    fn test_parse_candlesticks() {
        let json_data = load_test_json("http_get_candlesticks.json");
        let parsed: OKXResponse<OKXCandlestick> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 2);

        let bar0 = &parsed.data[0];
        assert_eq!(bar0.0, "1625097600000");
        assert_eq!(bar0.1, "33528.6");
        assert_eq!(bar0.2, "33870.0");
        assert_eq!(bar0.3, "33528.6");
        assert_eq!(bar0.4, "33783.9");
        assert_eq!(bar0.5, "778.838");

        let bar1 = &parsed.data[1];
        assert_eq!(bar1.0, "1625097660000");
        assert_eq!(bar1.1, "33783.9");
        assert_eq!(bar1.2, "33783.9");
        assert_eq!(bar1.3, "33782.1");
        assert_eq!(bar1.4, "33782.1");
        assert_eq!(bar1.5, "0.123");
    }

    #[rstest]
    fn test_parse_candlesticks_full() {
        let json_data = load_test_json("http_get_candlesticks_full.json");
        let parsed: OKXResponse<OKXCandlestick> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 2);

        // Inspect first record
        let bar0 = &parsed.data[0];
        assert_eq!(bar0.0, "1747094040000");
        assert_eq!(bar0.1, "102806.1");
        assert_eq!(bar0.2, "102820.4");
        assert_eq!(bar0.3, "102806.1");
        assert_eq!(bar0.4, "102820.4");
        assert_eq!(bar0.5, "1040.37");
        assert_eq!(bar0.6, "10.4037");
        assert_eq!(bar0.7, "1069603.34883");
        assert_eq!(bar0.8, "1");

        // Inspect second record
        let bar1 = &parsed.data[1];
        assert_eq!(bar1.0, "1747093980000");
        assert_eq!(bar1.5, "7164.04");
        assert_eq!(bar1.6, "71.6404");
        assert_eq!(bar1.7, "7364701.57952");
        assert_eq!(bar1.8, "1");
    }

    #[rstest]
    fn test_parse_mark_price() {
        let json_data = load_test_json("http_get_mark_price.json");
        let parsed: OKXResponse<OKXMarkPrice> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let mark_price = &parsed.data[0];

        assert_eq!(mark_price.inst_id, "BTC-USDT-SWAP");
        assert_eq!(mark_price.mark_px, "84660.1");
        assert_eq!(mark_price.ts, 1744590349506);
    }

    #[rstest]
    fn test_parse_index_price() {
        let json_data = load_test_json("http_get_index_price.json");
        let parsed: OKXResponse<OKXIndexTicker> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let index_price = &parsed.data[0];

        assert_eq!(index_price.inst_id, "BTC-USDT");
        assert_eq!(index_price.idx_px, "103895");
        assert_eq!(index_price.ts, 1746942707815);
    }

    #[rstest]
    fn test_parse_account() {
        let json_data = load_test_json("http_get_account_balance.json");
        let parsed: OKXResponse<OKXAccount> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let account = &parsed.data[0];
        assert_eq!(account.adj_eq, "");
        assert_eq!(account.borrow_froz, "");
        assert_eq!(account.imr, "");
        assert_eq!(account.iso_eq, "5.4682385526666675");
        assert_eq!(account.mgn_ratio, "");
        assert_eq!(account.mmr, "");
        assert_eq!(account.notional_usd, "");
        assert_eq!(account.notional_usd_for_borrow, "");
        assert_eq!(account.notional_usd_for_futures, "");
        assert_eq!(account.notional_usd_for_option, "");
        assert_eq!(account.notional_usd_for_swap, "");
        assert_eq!(account.ord_froz, "");
        assert_eq!(account.total_eq, "99.88870288820581");
        assert_eq!(account.upl, "");
        assert_eq!(account.u_time, 1744499648556);
        assert_eq!(account.details.len(), 1);

        let detail = &account.details[0];
        assert_eq!(detail.ccy, "USDT");
        assert_eq!(detail.avail_bal, "94.42612990333333");
        assert_eq!(detail.avail_eq, "94.42612990333333");
        assert_eq!(detail.cash_bal, "94.42612990333333");
        // assert_eq!(detail.collateral_enabled, false);  // TODO: Determine field
        assert_eq!(detail.dis_eq, "5.4682385526666675");
        assert_eq!(detail.eq, "99.89469657000001");
        assert_eq!(detail.eq_usd, "99.88870288820581");
        assert_eq!(detail.fixed_bal, "0");
        assert_eq!(detail.frozen_bal, "5.468566666666667");
        assert_eq!(detail.imr, "0");
        assert_eq!(detail.iso_eq, "5.468566666666667");
        assert_eq!(detail.iso_upl, "-0.0273000000000002");
        assert_eq!(detail.mmr, "0");
        assert_eq!(detail.notional_lever, "0");
        assert_eq!(detail.ord_frozen, "0");
        assert_eq!(detail.reward_bal, "0");
        assert_eq!(detail.smt_sync_eq, "0");
        assert_eq!(detail.spot_copy_trading_eq, "0");
        assert_eq!(detail.spot_iso_bal, "0");
        assert_eq!(detail.stgy_eq, "0");
        assert_eq!(detail.twap, "0");
        assert_eq!(detail.upl, "-0.0273000000000002");
        assert_eq!(detail.u_time, 1744498994783);
    }

    #[rstest]
    fn test_parse_order_history() {
        let json_data = load_test_json("http_get_orders_history.json");
        let parsed: OKXResponse<OKXOrderHistory> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let order = &parsed.data[0];
        assert_eq!(order.ord_id, "2497956918703120384");
        assert_eq!(order.fill_sz, "0.03");
        assert_eq!(order.acc_fill_sz, "0.03");
        assert_eq!(order.state, "filled");
        // fill_fee was omitted in response
        assert!(order.fill_fee.is_none());
    }

    #[rstest]
    fn test_parse_position() {
        let json_data = load_test_json("http_get_positions.json");
        let parsed: OKXResponse<OKXPosition> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let pos = &parsed.data[0];
        assert_eq!(pos.inst_id, "BTC-USDT-SWAP");
        assert_eq!(pos.pos_side, OKXPositionSide::Long);
        assert_eq!(pos.pos, "0.5");
        assert_eq!(pos.base_bal, "0.5");
        assert_eq!(pos.quote_bal, "5000");
        assert_eq!(pos.u_time, 1622559930237);
    }

    #[rstest]
    fn test_parse_position_history() {
        let json_data = load_test_json("http_get_account_positions-history.json");
        let parsed: OKXResponse<OKXPositionHistory> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first record
        let hist = &parsed.data[0];
        assert_eq!(hist.inst_id, "ETH-USDT-SWAP");
        assert_eq!(hist.inst_type, OKXInstrumentType::Swap);
        assert_eq!(hist.mgn_mode, OKXMarginMode::Isolated);
        assert_eq!(hist.pos_side, OKXPositionSide::Long);
        assert_eq!(hist.lever, "3.0");
        assert_eq!(hist.open_avg_px, "3226.93");
        assert_eq!(hist.close_avg_px.as_deref(), Some("3224.8"));
        assert_eq!(hist.pnl.as_deref(), Some("-0.0213"));
        assert!(!hist.c_time.is_empty());
        assert!(hist.u_time > 0);
    }

    #[rstest]
    fn test_parse_position_tiers() {
        let json_data = load_test_json("http_get_position_tiers.json");
        let parsed: OKXResponse<OKXPositionTier> = serde_json::from_str(&json_data).unwrap();

        // Basic response envelope
        assert_eq!(parsed.code, "0");
        assert_eq!(parsed.msg, "");
        assert_eq!(parsed.data.len(), 1);

        // Inspect first tier record
        let tier = &parsed.data[0];
        assert_eq!(tier.inst_id, "BTC-USDT");
        assert_eq!(tier.tier, "1");
        assert_eq!(tier.min_sz, "0");
        assert_eq!(tier.max_sz, "50");
        assert_eq!(tier.imr, "0.1");
        assert_eq!(tier.mmr, "0.03");
    }

    #[rstest]
    fn test_parse_account_field_name_compatibility() {
        // Test with new field names (with Amt suffix)
        let json_new = r#"{
            "accAvgPx": "",
            "availBal": "100.0",
            "availEq": "100.0",
            "borrowFroz": "",
            "cashBal": "100.0",
            "ccy": "USDT",
            "clSpotInUseAmt": "25.0",
            "crossLiab": "",
            "disEq": "0",
            "eq": "100.0",
            "eqUsd": "100.0",
            "fixedBal": "0",
            "frozenBal": "0",
            "imr": "0",
            "interest": "",
            "isoEq": "0",
            "isoLiab": "",
            "isoUpl": "0",
            "liab": "",
            "maxLoan": "",
            "maxSpotInUseAmt": "50.0",
            "mgnRatio": "",
            "mmr": "0",
            "notionalLever": "0",
            "openAvgPx": "",
            "ordFrozen": "0",
            "rewardBal": "0",
            "smtSyncEq": "0",
            "spotBal": "",
            "spotCopyTradingEq": "0",
            "spotInUseAmt": "30.0",
            "spotIsoBal": "0",
            "spotUpl": "",
            "spotUplRatio": "",
            "stgyEq": "0",
            "totalPnl": "",
            "totalPnlRatio": "",
            "twap": "0",
            "uTime": "1234567890",
            "upl": "0",
            "uplLiab": ""
        }"#;

        let detail_new: OKXBalanceDetail = serde_json::from_str(json_new).unwrap();
        assert_eq!(detail_new.max_spot_in_use_amt, "50.0");
        assert_eq!(detail_new.spot_in_use_amt, "30.0");
        assert_eq!(detail_new.cl_spot_in_use_amt, "25.0");

        // Test with old field names (without Amt suffix) - for backward compatibility
        let json_old = r#"{
            "accAvgPx": "",
            "availBal": "100.0",
            "availEq": "100.0",
            "borrowFroz": "",
            "cashBal": "100.0",
            "ccy": "USDT",
            "clSpotInUse": "35.0",
            "crossLiab": "",
            "disEq": "0",
            "eq": "100.0",
            "eqUsd": "100.0",
            "fixedBal": "0",
            "frozenBal": "0",
            "imr": "0",
            "interest": "",
            "isoEq": "0",
            "isoLiab": "",
            "isoUpl": "0",
            "liab": "",
            "maxLoan": "",
            "maxSpotInUse": "75.0",
            "mgnRatio": "",
            "mmr": "0",
            "notionalLever": "0",
            "openAvgPx": "",
            "ordFrozen": "0",
            "rewardBal": "0",
            "smtSyncEq": "0",
            "spotBal": "",
            "spotCopyTradingEq": "0",
            "spotInUse": "40.0",
            "spotIsoBal": "0",
            "spotUpl": "",
            "spotUplRatio": "",
            "stgyEq": "0",
            "totalPnl": "",
            "totalPnlRatio": "",
            "twap": "0",
            "uTime": "1234567890",
            "upl": "0",
            "uplLiab": ""
        }"#;

        let detail_old: OKXBalanceDetail = serde_json::from_str(json_old).unwrap();
        assert_eq!(detail_old.max_spot_in_use_amt, "75.0");
        assert_eq!(detail_old.spot_in_use_amt, "40.0");
        assert_eq!(detail_old.cl_spot_in_use_amt, "35.0");
    }

    #[rstest]
    fn test_parse_place_order_response() {
        let json_data = r#"{
            "ordId": "12345678901234567890",
            "clOrdId": "client_order_123",
            "tag": "",
            "sCode": "0",
            "sMsg": ""
        }"#;

        let parsed: OKXPlaceOrderResponse = serde_json::from_str(json_data).unwrap();
        assert_eq!(
            parsed.ord_id,
            Some(ustr::Ustr::from("12345678901234567890"))
        );
        assert_eq!(parsed.cl_ord_id, Some(ustr::Ustr::from("client_order_123")));
        assert_eq!(parsed.tag, Some("".to_string()));
    }

    #[rstest]
    fn test_parse_transaction_details() {
        let json_data = r#"{
            "instType": "SPOT",
            "instId": "BTC-USDT",
            "tradeId": "123456789",
            "ordId": "987654321",
            "clOrdId": "client_123",
            "billId": "bill_456",
            "fillPx": "42000.5",
            "fillSz": "0.001",
            "side": "buy",
            "execType": "T",
            "feeCcy": "USDT",
            "fee": "0.042",
            "ts": "1625097600000"
        }"#;

        let parsed: OKXTransactionDetail = serde_json::from_str(json_data).unwrap();
        assert_eq!(parsed.inst_type, OKXInstrumentType::Spot);
        assert_eq!(parsed.inst_id, Ustr::from("BTC-USDT"));
        assert_eq!(parsed.trade_id, Ustr::from("123456789"));
        assert_eq!(parsed.ord_id, Ustr::from("987654321"));
        assert_eq!(parsed.cl_ord_id, Ustr::from("client_123"));
        assert_eq!(parsed.bill_id, Ustr::from("bill_456"));
        assert_eq!(parsed.fill_px, "42000.5");
        assert_eq!(parsed.fill_sz, "0.001");
        assert_eq!(parsed.side, OKXSide::Buy);
        assert_eq!(parsed.exec_type, OKXExecType::Taker);
        assert_eq!(parsed.fee_ccy, "USDT");
        assert_eq!(parsed.fee, Some("0.042".to_string()));
        assert_eq!(parsed.ts, 1625097600000);
    }

    #[rstest]
    fn test_parse_empty_fee_field() {
        use crate::http::models::OKXTransactionDetail;

        let json_data = r#"{
            "instType": "SPOT",
            "instId": "BTC-USDT",
            "tradeId": "123456789",
            "ordId": "987654321",
            "clOrdId": "client_123",
            "billId": "bill_456",
            "fillPx": "42000.5",
            "fillSz": "0.001",
            "side": "buy",
            "execType": "T",
            "feeCcy": "USDT",
            "fee": "",
            "ts": "1625097600000"
        }"#;

        let parsed: OKXTransactionDetail = serde_json::from_str(json_data).unwrap();
        assert_eq!(parsed.fee, None);
    }

    #[rstest]
    fn test_parse_optional_string_to_u64() {
        use serde::Deserialize;

        #[derive(Deserialize)]
        struct TestStruct {
            #[serde(deserialize_with = "crate::common::parse::deserialize_optional_string_to_u64")]
            value: Option<u64>,
        }

        // Test with valid string
        let json_value = r#"{"value": "12345"}"#;
        let result: TestStruct = serde_json::from_str(json_value).unwrap();
        assert_eq!(result.value, Some(12345));

        // Test with empty string
        let json_empty = r#"{"value": ""}"#;
        let result_empty: TestStruct = serde_json::from_str(json_empty).unwrap();
        assert_eq!(result_empty.value, None);

        // Test with null
        let json_null = r#"{"value": null}"#;
        let result_null: TestStruct = serde_json::from_str(json_null).unwrap();
        assert_eq!(result_null.value, None);
    }

    #[rstest]
    fn test_parse_error_handling() {
        // Test error handling with invalid price string
        let invalid_price = "invalid-price";
        let result = crate::common::parse::parse_price(invalid_price, 2);
        assert!(result.is_err());

        // Test error handling with invalid quantity string
        let invalid_quantity = "invalid-quantity";
        let result = crate::common::parse::parse_quantity(invalid_quantity, 8);
        assert!(result.is_err());
    }
}
