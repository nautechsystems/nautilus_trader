// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use nautilus_bitget::websocket::messages::{
    BitgetWsPrivateAccount, BitgetWsPrivateChannelMessage, BitgetWsPrivateFill,
    BitgetWsPrivateOrder, BitgetWsPrivatePosition,
};

#[test]
fn deserialize_private_account_channel_message() {
    let payload = r#"{
        "action":"snapshot",
        "arg":{"instType":"SPOT","channel":"account","coin":"default"},
        "data":[
            {
                "coin":"USDT",
                "available":"100.5",
                "frozen":"2.0",
                "locked":"1.0",
                "limitAvailable":"97.5",
                "uTime":"1708883200123"
            }
        ]
    }"#;

    let msg: BitgetWsPrivateChannelMessage<BitgetWsPrivateAccount> =
        serde_json::from_str(payload).expect("account message should deserialize");

    assert_eq!(msg.action, "snapshot");
    assert_eq!(msg.data[0].coin, "USDT");
    assert_eq!(msg.data[0].available, "100.5");
}

#[test]
fn deserialize_private_order_channel_message() {
    let payload = r#"{
        "action":"snapshot",
        "arg":{"instType":"SPOT","channel":"orders","instId":"default"},
        "data":[
            {
                "instId":"BTCUSDT",
                "orderId":"12345",
                "clientOid":"client-1",
                "price":"45000",
                "size":"0.01",
                "fillPrice":"44995",
                "fillQuantity":"0.005",
                "fillFee":"0.1",
                "fillFeeCoin":"USDT",
                "tradeId":"t-1",
                "side":"buy",
                "orderType":"limit",
                "force":"gtc",
                "accBaseVolume":"0.005",
                "priceAvg":"44995",
                "status":"partial-fill",
                "cTime":"1708883200000",
                "uTime":"1708883200123",
                "feeDetail":[{"feeCoin":"USDT","fee":"0.1"}]
            }
        ]
    }"#;

    let msg: BitgetWsPrivateChannelMessage<BitgetWsPrivateOrder> =
        serde_json::from_str(payload).expect("order message should deserialize");

    assert_eq!(msg.data[0].order_id, "12345");
    assert_eq!(msg.data[0].client_oid, "client-1");
    assert_eq!(msg.data[0].fee_detail[0].fee_coin, "USDT");
}

#[test]
fn deserialize_private_fill_channel_message() {
    let payload = r#"{
        "action":"snapshot",
        "arg":{"instType":"SPOT","channel":"fill","instId":"default"},
        "data":[
            {
                "orderId":"12345",
                "tradeId":"t-1",
                "symbol":"BTCUSDT",
                "orderType":"limit",
                "side":"buy",
                "priceAvg":"44995",
                "size":"0.005",
                "amount":"224.975",
                "tradeScope":"taker",
                "feeDetail":[
                    {
                        "feeCoin":"USDT",
                        "deduction":"0",
                        "totalDeductionFee":"0",
                        "totalFee":"0.1"
                    }
                ],
                "cTime":"1708883200000",
                "uTime":"1708883200123"
            }
        ]
    }"#;

    let msg: BitgetWsPrivateChannelMessage<BitgetWsPrivateFill> =
        serde_json::from_str(payload).expect("fill message should deserialize");

    assert_eq!(msg.data[0].trade_id, "t-1");
    assert_eq!(msg.data[0].fee_detail[0].total_fee, "0.1");
}

#[test]
fn deserialize_private_position_channel_message() {
    let payload = r#"{
        "action":"snapshot",
        "arg":{"instType":"USDT-FUTURES","channel":"positions","instId":"default"},
        "data":[
            {
                "posId":"p-1",
                "instId":"BTCUSDT",
                "marginCoin":"USDT",
                "marginSize":"100",
                "marginMode":"crossed",
                "holdSide":"long",
                "posMode":"one_way_mode",
                "total":"0.01",
                "available":"0.01",
                "frozen":"0",
                "openPriceAvg":"45000",
                "leverage":"10",
                "unrealizedPL":"5",
                "liquidationPrice":"40000",
                "markPrice":"45500",
                "uTime":"1708883200123"
            }
        ]
    }"#;

    let msg: BitgetWsPrivateChannelMessage<BitgetWsPrivatePosition> =
        serde_json::from_str(payload).expect("position message should deserialize");

    assert_eq!(msg.data[0].pos_id, "p-1");
    assert_eq!(msg.data[0].hold_side, "long");
    assert_eq!(msg.data[0].mark_price, "45500");
}
