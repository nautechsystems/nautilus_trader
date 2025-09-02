//! websocket/exchange/hyperliquid.rs
//! Thin adapter for Hyperliquid WS, translating between your canonical
//! WsOutbound/WsInbound and Hyperliquid's { "method": ... } wire JSON.
//!
//! Notes:
//! - Hyperliquid accepts exactly **one** subscription per frame. If you batch
//!   Subscribe/Unsubscribe, `encode` returns `Value::Array([...])`; your writer
//!   should send each element as its own frame.
//! - We synthesize `is_snapshot = true` on the **first** l2Book push per
//!   (channel|symbol|params) after (re)subscribe.
//! - No floats: all numerics parsed to `rust_decimal::Decimal`.

use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
};

use rust_decimal::Decimal;
use serde_json::{Value, json};

use super::adapter::ExchangeAdapter;
use crate::websocket::codec::{
    Level, Notice, PostAck, Side, SubArg, SubResp, WsBbo, WsBook, WsCandle, WsFill, WsFunding,
    WsInbound, WsMid, WsOutbound, WsTrade,
};

/// Hyperliquid adapter implementing your `ExchangeAdapter` contract.
#[derive(Default, Debug)]
pub struct HyperliquidAdapter {
    // Tracks first emission per topic to synthesize `is_snapshot`
    first_seen: Arc<Mutex<HashSet<String>>>,
}

impl HyperliquidAdapter {
    pub fn new() -> Self {
        Self::default()
    }

    fn topic_key(&self, ch: &str, sym: Option<&str>, extra: Option<&Value>) -> String {
        let mut k = ch.to_string();
        if let Some(s) = sym {
            k.push('|');
            k.push_str(s);
        }
        if let Some(e) = extra {
            k.push('|');
            k.push_str(&serde_json::to_string(e).unwrap_or_default());
        }
        k
    }

    fn mark_first(&self, key: &str) -> bool {
        let mut g = self.first_seen.lock().unwrap();
        if g.contains(key) {
            false
        } else {
            g.insert(key.to_string());
            true
        }
    }

    // ---- Outbound mapping helpers -------------------------------------------------------------

    fn build_sub(&self, r: &SubArg) -> Value {
        // { "method":"subscribe", "subscription": { "type": <channel>, ... } }
        let mut sub = serde_json::Map::new();
        sub.insert("type".into(), Value::String(r.channel.clone()));

        if let Some(sym) = &r.symbol {
            // Public market channels use "coin"
            match r.channel.as_str() {
                "l2Book" | "trades" | "bbo" | "candle" | "activeAssetCtx" | "activeAssetData" => {
                    sub.insert("coin".into(), Value::String(sym.clone()));
                }
                _ => { /* private channels: user/address/etc. go via params */ }
            }
        }
        if let Some(p) = &r.params
            && let Some(obj) = p.as_object()
        {
            for (k, v) in obj {
                sub.insert(k.clone(), v.clone());
            }
        }
        json!({ "method":"subscribe", "subscription": sub })
    }

    fn build_unsub(&self, r: &SubArg) -> Value {
        let mut sub = serde_json::Map::new();
        sub.insert("type".into(), Value::String(r.channel.clone()));
        if let Some(sym) = &r.symbol {
            match r.channel.as_str() {
                "l2Book" | "trades" | "bbo" | "candle" | "activeAssetCtx" | "activeAssetData" => {
                    sub.insert("coin".into(), Value::String(sym.clone()));
                }
                _ => {}
            }
        }
        if let Some(p) = &r.params
            && let Some(obj) = p.as_object()
        {
            for (k, v) in obj {
                sub.insert(k.clone(), v.clone());
            }
        }
        json!({ "method":"unsubscribe", "subscription": sub })
    }

    fn build_post(&self, id: &str, post_type: &str, payload: &Value) -> Value {
        // { "method":"post", "id": <u64>, "request": { "type":"info"|"action", "payload":{...} } }
        let id_num = id.parse::<u64>().unwrap_or_else(|_| {
            use std::hash::{Hash, Hasher};
            let mut h = std::collections::hash_map::DefaultHasher::new();
            id.hash(&mut h);
            h.finish() & 0x7fff_ffff_ffff_ffff
        });
        json!({
            "method":"post",
            "id": id_num,
            "request": { "type": post_type, "payload": payload }
        })
    }
}

impl ExchangeAdapter for HyperliquidAdapter {
    /// Map canonical outbound → Hyperliquid wire JSON.
    /// IMPORTANT: for batched Subscribe/Unsubscribe we return `Value::Array([...])`.
    fn encode(&self, msg: &WsOutbound) -> Value {
        match msg {
            WsOutbound::Subscribe { args, id: _ } => {
                let frames: Vec<_> = args.iter().map(|r| self.build_sub(r)).collect();
                if frames.len() == 1 {
                    frames[0].clone()
                } else {
                    Value::Array(frames)
                }
            }
            WsOutbound::Unsubscribe { args, id: _ } => {
                let frames: Vec<_> = args.iter().map(|r| self.build_unsub(r)).collect();
                if frames.len() == 1 {
                    frames[0].clone()
                } else {
                    Value::Array(frames)
                }
            }
            WsOutbound::Ping => json!({ "method":"ping" }), // server replies channel:"pong"
            WsOutbound::Post { id, path: _, body } => self.build_post(id, "action", body),
            // HL doesn't have a separate WS login; signed actions go via `post`.
            WsOutbound::Auth { payload } => self.build_post("auth", "action", payload),
        }
    }

    /// Map Hyperliquid wire JSON text → canonical inbound.
    fn decode(&self, txt: &str) -> Option<WsInbound> {
        let v: Value = serde_json::from_str(txt).ok()?;
        let ch = v
            .get("channel")
            .and_then(|x| x.as_str())
            .unwrap_or_default();

        match ch {
            // ---- Control ----
            "subscriptionResponse" => Some(WsInbound::SubscriptionResponse(SubResp {
                id: None,
                ok: true, // HL acks surface errors via "notification"; assume success here
                message: None,
            })),
            "pong" => Some(WsInbound::Pong(None)),
            "notification" => {
                let code = v.get("code").map(|x| x.to_string());
                let msg = v.get("msg").and_then(|x| x.as_str()).map(|s| s.to_string());
                Some(WsInbound::Notification(Notice { code, msg }))
            }
            "post" => {
                // { channel:"post", data:{ id, response:{ type:"info"|"action"|"error", payload:{...} } } }
                let id = v
                    .pointer("/data/id")
                    .and_then(|x| x.as_i64())
                    .map(|n| n.to_string())
                    .unwrap_or_else(|| "0".into());
                let rtype = v
                    .pointer("/data/response/type")
                    .and_then(|x| x.as_str())
                    .unwrap_or("error");
                let err = if rtype == "error" {
                    v.pointer("/data/response/payload").map(|x| x.to_string())
                } else {
                    None
                };
                Some(WsInbound::Post(PostAck {
                    id,
                    ok: rtype != "error",
                    error: err,
                }))
            }

            // ---- Public market data ----
            "allMids" => {
                // { data: { mids: { "BTC":"12345.6", ... } } }
                let mids = v.pointer("/data/mids")?.as_object()?;
                let out = mids
                    .iter()
                    .map(|(coin, val)| WsMid {
                        symbol: coin.clone(),
                        mid: val
                            .as_str()
                            .unwrap_or("0")
                            .parse::<Decimal>()
                            .unwrap_or(Decimal::ZERO),
                        ts: None,
                    })
                    .collect::<Vec<_>>();
                Some(WsInbound::AllMids(out))
            }
            "trades" => {
                let arr = v.get("data")?.as_array()?;
                let mut out = Vec::with_capacity(arr.len());
                for t in arr {
                    let sym = t
                        .get("coin")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let px = t
                        .get("px")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO);
                    let sz = t
                        .get("sz")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO);
                    let side = match t.get("side").and_then(|x| x.as_str()).unwrap_or("buy") {
                        "sell" | "SELL" => Side::Sell,
                        _ => Side::Buy,
                    };
                    let ts = t.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
                    let id = t.get("tid").map(|x| x.to_string());
                    out.push(WsTrade {
                        instrument: sym,
                        px,
                        qty: sz,
                        side,
                        ts,
                        id,
                    });
                }
                Some(WsInbound::Trades(out))
            }
            "l2Book" => {
                // { data: { coin, time, levels: [ bids[], asks[] ] } }
                let d = v.get("data")?;
                let sym = d
                    .get("coin")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string();

                static EMPTY_VEC: Vec<Value> = Vec::new();
                let bids = d
                    .pointer("/levels/0")
                    .and_then(|x| x.as_array())
                    .unwrap_or(&EMPTY_VEC);
                let asks = d
                    .pointer("/levels/1")
                    .and_then(|x| x.as_array())
                    .unwrap_or(&EMPTY_VEC);
                let to_levels = |side: &Vec<Value>| {
                    side.iter()
                        .map(|l| {
                            let px = l
                                .get("px")
                                .and_then(|x| x.as_str())
                                .unwrap_or("0")
                                .parse::<Decimal>()
                                .unwrap_or(Decimal::ZERO);
                            let sz = l
                                .get("sz")
                                .and_then(|x| x.as_str())
                                .unwrap_or("0")
                                .parse::<Decimal>()
                                .unwrap_or(Decimal::ZERO);
                            Level { px, qty: sz }
                        })
                        .collect::<Vec<_>>()
                };
                let key = self.topic_key("l2Book", Some(&sym), None);
                let is_snapshot = self.mark_first(&key); // synthesize first-only
                let ts = d.get("time").and_then(|x| x.as_i64());
                Some(WsInbound::L2Book(WsBook {
                    instrument: sym,
                    is_snapshot,
                    seq: None,
                    checksum: None,
                    bids: to_levels(bids),
                    asks: to_levels(asks),
                    ts: ts.unwrap_or(0),
                }))
            }
            "bbo" => {
                // { data: { coin, time, bbo:[ bid|null, ask|null ] } }
                let d = v.get("data")?;
                let sym = d
                    .get("coin")
                    .and_then(|x| x.as_str())
                    .unwrap_or_default()
                    .to_string();
                let ts = d.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
                let (mut bid_px, mut bid_qty, mut ask_px, mut ask_qty) =
                    (Decimal::ZERO, Decimal::ZERO, Decimal::ZERO, Decimal::ZERO);
                if let Some(b) = d.pointer("/bbo/0").and_then(|x| x.as_object()) {
                    bid_px = b
                        .get("px")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(Decimal::ZERO);
                    bid_qty = b
                        .get("sz")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(Decimal::ZERO);
                }
                if let Some(a) = d.pointer("/bbo/1").and_then(|x| x.as_object()) {
                    ask_px = a
                        .get("px")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(Decimal::ZERO);
                    ask_qty = a
                        .get("sz")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse()
                        .unwrap_or(Decimal::ZERO);
                }
                Some(WsInbound::Bbo(WsBbo {
                    instrument: sym,
                    bid_px,
                    bid_qty,
                    ask_px,
                    ask_qty,
                    ts,
                }))
            }
            "candle" => {
                // [ { s:"BTC", i:"1m", t:<open_ts>, o,h,l,c,v }, ... ]
                let arr = v.get("data")?.as_array()?;
                let mut out = Vec::with_capacity(arr.len());
                for c in arr {
                    let sym = c
                        .get("s")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let interval = c
                        .get("i")
                        .and_then(|x| x.as_str())
                        .unwrap_or("1m")
                        .to_string();
                    let t_open = c.get("t").and_then(|x| x.as_i64()).unwrap_or(0);
                    let o = dec(c.get("o"));
                    let h = dec(c.get("h"));
                    let l = dec(c.get("l"));
                    let close = dec(c.get("c"));
                    let v_ = dec(c.get("v"));
                    out.push(WsCandle {
                        instrument: sym,
                        interval,
                        open_ts: t_open,
                        o,
                        h,
                        l,
                        c: close,
                        v: v_,
                    });
                }
                Some(WsInbound::Candle(out))
            }

            // ---- Private (examples) ----
            "userFills" => {
                let d = v.get("data")?;
                static EMPTY_VEC: Vec<Value> = Vec::new();
                let fills = d
                    .get("fills")
                    .and_then(|x| x.as_array())
                    .unwrap_or(&EMPTY_VEC);
                let mut out = Vec::with_capacity(fills.len());
                for f in fills {
                    let sym = f
                        .get("coin")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let px = f
                        .get("px")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO);
                    let sz = f
                        .get("sz")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO);
                    let side = match f.get("side").and_then(|x| x.as_str()).unwrap_or("buy") {
                        "sell" | "SELL" => Side::Sell,
                        _ => Side::Buy,
                    };
                    let ts = f.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
                    let order_id = f.get("oid").map(|x| x.to_string()).unwrap_or_default();
                    let tid = f.get("tid").map(|x| x.to_string()).unwrap_or_default();
                    out.push(WsFill {
                        symbol: sym,
                        order_id,
                        trade_id: tid,
                        px,
                        qty: sz,
                        side,
                        ts,
                    });
                }
                Some(WsInbound::UserFills(out))
            }
            "userFundings" => {
                let d = v.get("data")?;
                static EMPTY_VEC: Vec<Value> = Vec::new();
                let events = d
                    .get("events")
                    .and_then(|x| x.as_array())
                    .unwrap_or(&EMPTY_VEC);
                let mut out = Vec::with_capacity(events.len());
                for e in events {
                    let sym = e
                        .get("coin")
                        .and_then(|x| x.as_str())
                        .unwrap_or_default()
                        .to_string();
                    let rate = e
                        .get("rate")
                        .and_then(|x| x.as_str())
                        .unwrap_or("0")
                        .parse::<Decimal>()
                        .unwrap_or(Decimal::ZERO);
                    let ts = e.get("time").and_then(|x| x.as_i64()).unwrap_or(0);
                    out.push(WsFunding {
                        symbol: sym,
                        rate,
                        ts,
                    });
                }
                Some(WsInbound::UserFundings(out))
            }

            _ => Some(WsInbound::Unknown),
        }
    }
}

// small helper to parse string-or-number to Decimal
fn dec(v: Option<&Value>) -> Decimal {
    match v {
        Some(Value::String(s)) => s.parse().unwrap_or(Decimal::ZERO),
        Some(Value::Number(n)) => n.to_string().parse().unwrap_or(Decimal::ZERO),
        _ => Decimal::ZERO,
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn sub(ch: &str, sym: Option<&str>, params: Option<Value>) -> SubArg {
        SubArg {
            channel: ch.to_string(),
            symbol: sym.map(|s| s.to_string()),
            params,
        }
    }

    #[rstest]
    fn encode_single_and_batched_subscribe() {
        let a = HyperliquidAdapter::new();

        // single sub → single object
        let v1 = a.encode(&WsOutbound::Subscribe {
            args: vec![sub("trades", Some("ETH"), None)],
            id: None,
        });
        assert!(v1.is_object());
        assert_eq!(v1["method"], "subscribe");
        assert_eq!(v1["subscription"]["type"], "trades");
        assert_eq!(v1["subscription"]["coin"], "ETH");

        // batched subs → array
        let v2 = a.encode(&WsOutbound::Subscribe {
            args: vec![
                sub("trades", Some("ETH"), None),
                sub("l2Book", Some("ETH"), None),
            ],
            id: None,
        });
        assert!(v2.is_array());
        assert_eq!(v2.as_array().unwrap().len(), 2);
    }

    #[rstest]
    fn decode_trades() {
        let a = HyperliquidAdapter::new();
        let wire = json!({
          "channel":"trades",
          "data":[
            {"coin":"ETH","side":"buy","px":"3000.1","sz":"0.5","time":1715931000000i64,"tid":42},
            {"coin":"ETH","side":"sell","px":"3000.0","sz":"0.3","time":1715931000500i64,"tid":43}
          ]
        })
        .to_string();

        match a.decode(&wire).expect("decoded") {
            WsInbound::Trades(ts) => {
                assert_eq!(ts.len(), 2);
                assert_eq!(ts[0].instrument, "ETH");
                assert_eq!(ts[0].px.to_string(), "3000.1");
                assert!(matches!(ts[1].side, Side::Sell));
            }
            other => panic!("unexpected variant: {:?}", other),
        }
    }

    #[rstest]
    fn decode_l2book_and_bbo() {
        let a = HyperliquidAdapter::new();

        // First l2Book → snapshot synthesized
        let book = json!({
          "channel":"l2Book",
          "data":{"coin":"BTC","time":1715931000000i64,"levels":[
            [ {"px":"60000","sz":"1.0"} ],
            [ {"px":"60010","sz":"2.0"} ]
          ]}
        })
        .to_string();

        match a.decode(&book).expect("decoded") {
            WsInbound::L2Book(b) => {
                assert_eq!(b.instrument, "BTC");
                assert!(b.is_snapshot);
                assert_eq!(b.bids[0].px.to_string(), "60000");
                assert_eq!(b.asks[0].qty.to_string(), "2.0");
            }
            _ => panic!("book"),
        }

        // bbo
        let bbo = json!({
          "channel":"bbo",
          "data":{"coin":"BTC","time":1715931000100i64,"bbo":[
            {"px":"59990","sz":"0.8"},
            {"px":"60005","sz":"1.2"}
          ]}
        })
        .to_string();

        match a.decode(&bbo).expect("decoded") {
            WsInbound::Bbo(t) => {
                assert_eq!(t.instrument, "BTC");
                assert_eq!(t.bid_px.to_string(), "59990");
                assert_eq!(t.ask_px.to_string(), "60005");
            }
            _ => panic!("bbo"),
        }
    }

    #[rstest]
    fn control_frames_pong_and_post_ack() {
        let a = HyperliquidAdapter::new();

        let pong = r#"{ "channel":"pong" }"#;
        match a.decode(pong).unwrap() {
            WsInbound::Pong(_) => {}
            _ => panic!("expected pong"),
        }

        let post_ok = json!({
          "channel":"post",
          "data": { "id": 123, "response": { "type":"info", "payload": { "ok": true } } }
        })
        .to_string();
        match a.decode(&post_ok).unwrap() {
            WsInbound::Post(PostAck { id, ok, error }) => {
                assert_eq!(id, "123");
                assert!(ok);
                assert!(error.is_none());
            }
            _ => panic!("expected post ack"),
        }

        let post_err = json!({
          "channel":"post",
          "data": { "id": 9, "response": { "type":"error", "payload": {"code": 400, "msg": "bad"} } }
        })
        .to_string();
        match a.decode(&post_err).unwrap() {
            WsInbound::Post(PostAck { id, ok, error }) => {
                assert_eq!(id, "9");
                assert!(!ok);
                assert!(error.unwrap().contains("bad"));
            }
            _ => panic!("expected post error"),
        }
    }
}
