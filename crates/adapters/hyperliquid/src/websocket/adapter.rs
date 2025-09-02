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

use std::{
    collections::HashSet,
    error::Error as StdError,
    fmt,
    sync::{Arc, Mutex},
};

use rust_decimal::Decimal;
use serde_json::{self, Value, json};
use url::Url;

use crate::websocket::codec::{
    Level, Notice, PostAck, Side, SubArg, SubResp, WsBbo, WsBook, WsCandle, WsInbound, WsMid,
    WsOutbound, WsTrade,
};

/// Errors for the Hyperliquid-only codec.
#[derive(Debug)]
#[non_exhaustive]
pub enum HyperliquidError {
    /// Provided WebSocket URL is malformed.
    MalformedUrl,
    /// URL is well-formed but does not appear to be a Hyperliquid endpoint.
    NotHyperliquidHost(String),
    /// Frame is not valid JSON.
    MalformedJson(String),
    /// Frame is valid JSON but not relevant/recognized (e.g., heartbeat).
    UnrecognizedFrame,
    /// Feature not supported (e.g., binary frames).
    Unsupported(&'static str),
}

impl fmt::Display for HyperliquidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HyperliquidError::MalformedUrl => write!(f, "malformed WebSocket URL"),
            HyperliquidError::NotHyperliquidHost(h) => write!(f, "not a Hyperliquid host: {}", h),
            HyperliquidError::MalformedJson(e) => write!(f, "malformed JSON: {}", e),
            HyperliquidError::UnrecognizedFrame => write!(f, "unrecognized frame"),
            HyperliquidError::Unsupported(m) => write!(f, "unsupported: {}", m),
        }
    }
}
impl StdError for HyperliquidError {}

/// Lightweight, monomorphic codec for Hyperliquid.
#[derive(Debug, Default)]
pub struct HyperliquidCodec {
    // Tracks first emission per topic to synthesize `is_snapshot`
    first_seen: Arc<Mutex<HashSet<String>>>,
}

impl HyperliquidCodec {
    /// Cheap constructor.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }

    /// Optional: sanity-check that a given URL points to Hyperliquid
    /// before you open a socket. You can skip this if the caller already knows.
    pub fn assert_hyperliquid_url(url: &str) -> Result<(), HyperliquidError> {
        let parsed = Url::parse(url).map_err(|_| HyperliquidError::MalformedUrl)?;
        let host = parsed.host_str().ok_or(HyperliquidError::MalformedUrl)?;

        // Keep this liberal; tighten if you want exact host matching.
        if host.contains("hyperliquid") {
            Ok(())
        } else {
            Err(HyperliquidError::NotHyperliquidHost(host.to_string()))
        }
    }

    /// Encode a canonical outbound message into one or more Hyperliquid JSON frames.
    ///
    /// Returning `Vec<Value>` makes multi-frame semantics explicit.
    pub fn encode(&self, msg: &WsOutbound) -> Result<Vec<Value>, HyperliquidError> {
        match msg {
            WsOutbound::Subscribe { args, id: _ } => {
                let frames: Vec<_> = args.iter().map(|r| self.build_sub(r)).collect();
                Ok(frames)
            }
            WsOutbound::Unsubscribe { args, id: _ } => {
                let frames: Vec<_> = args.iter().map(|r| self.build_unsub(r)).collect();
                Ok(frames)
            }
            WsOutbound::Ping => Ok(vec![json!({ "method":"ping" })]),
            WsOutbound::Post { id, path: _, body } => Ok(vec![self.build_post(id, "action", body)]),
            WsOutbound::Auth { payload } => Ok(vec![self.build_post("auth", "action", payload)]),
        }
    }

    /// Decode a Hyperliquid **text** frame into zero or more canonical inbound messages.
    ///
    /// Return `Ok(vec![])` for benign but irrelevant frames (heartbeats).
    pub fn decode_text(&self, txt: &str) -> Result<Vec<WsInbound>, HyperliquidError> {
        let v: Value = serde_json::from_str(txt)
            .map_err(|e| HyperliquidError::MalformedJson(e.to_string()))?;

        let ch = v
            .get("channel")
            .and_then(|x| x.as_str())
            .unwrap_or_default();

        let result = match ch {
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
            "allMids" => self.decode_all_mids(&v),
            "trades" => self.decode_trades(&v),
            "l2Book" => self.decode_l2book(&v),
            "bbo" => self.decode_bbo(&v),
            "candle" => self.decode_candle(&v),

            // ---- Private data ----
            "userFills" => self.decode_user_fills(&v),
            "userFundings" => self.decode_user_fundings(&v),
            "userEvents" => self.decode_user_events(&v),

            _ => None,
        };

        match result {
            Some(inbound) => Ok(vec![inbound]),
            None => Err(HyperliquidError::UnrecognizedFrame),
        }
    }

    /// Decode a Hyperliquid **binary** frame (default: unsupported).
    #[allow(unused_variables)]
    pub fn decode_binary(&self, bin: &[u8]) -> Result<Vec<WsInbound>, HyperliquidError> {
        Err(HyperliquidError::Unsupported(
            "binary frames not supported by HyperliquidCodec",
        ))
    }

    /// A stable name you can use in logs/metrics.
    #[inline]
    pub fn name(&self) -> &'static str {
        "hyperliquid"
    }

    // ---- Private helper methods for encoding ----

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

    // ---- Private helper methods for decoding ----

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

    fn decode_all_mids(&self, v: &Value) -> Option<WsInbound> {
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

    fn decode_trades(&self, v: &Value) -> Option<WsInbound> {
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

    fn decode_l2book(&self, v: &Value) -> Option<WsInbound> {
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

    fn decode_bbo(&self, v: &Value) -> Option<WsInbound> {
        // BBO (best bid/offer) implementation - placeholder
        let d = v.get("data")?;
        let sym = d
            .get("coin")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();

        // This is a placeholder - implement based on actual Hyperliquid BBO format
        Some(WsInbound::Bbo(WsBbo {
            instrument: sym,
            bid_px: Decimal::ZERO,
            bid_qty: Decimal::ZERO,
            ask_px: Decimal::ZERO,
            ask_qty: Decimal::ZERO,
            ts: 0,
        }))
    }

    fn decode_candle(&self, v: &Value) -> Option<WsInbound> {
        // Candle implementation - placeholder
        let d = v.get("data")?;
        let sym = d
            .get("coin")
            .and_then(|x| x.as_str())
            .unwrap_or_default()
            .to_string();

        // This is a placeholder - implement based on actual Hyperliquid candle format
        Some(WsInbound::Candle(vec![WsCandle {
            instrument: sym,
            interval: "1m".to_string(),
            open_ts: 0,
            o: Decimal::ZERO,
            h: Decimal::ZERO,
            l: Decimal::ZERO,
            c: Decimal::ZERO,
            v: Decimal::ZERO,
        }]))
    }

    fn decode_user_fills(&self, _v: &Value) -> Option<WsInbound> {
        // User fills implementation - placeholder
        Some(WsInbound::UserFills(vec![]))
    }

    fn decode_user_fundings(&self, _v: &Value) -> Option<WsInbound> {
        // User fundings implementation - placeholder
        Some(WsInbound::UserFundings(vec![]))
    }

    fn decode_user_events(&self, _v: &Value) -> Option<WsInbound> {
        // User events implementation - placeholder
        Some(WsInbound::UserEvents(vec![]))
    }
}

impl Clone for HyperliquidCodec {
    fn clone(&self) -> Self {
        Self {
            first_seen: Arc::new(Mutex::new(HashSet::new())),
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    /// Test cases for URL validation - ensuring we properly reject non-Hyperliquid URLs
    /// while accepting valid Hyperliquid endpoints
    #[rstest]
    #[case(
        "wss://example.com/ws",
        false,
        "standard websocket URL should be rejected"
    )]
    #[case("wss://binance.com/ws", false, "binance URL should be rejected")]
    #[case(
        "wss://ws.hyperliquid.xyz",
        true,
        "hyperliquid xyz domain should be accepted"
    )]
    #[case(
        "wss://api.hyperliquid.io/ws",
        true,
        "hyperliquid io domain should be accepted"
    )]
    #[case(
        "ws://hyperliquid.test",
        true,
        "hyperliquid test domain should be accepted"
    )]
    fn test_url_validation(
        #[case] url: &str,
        #[case] should_pass: bool,
        #[case] description: &str,
    ) {
        let result = HyperliquidCodec::assert_hyperliquid_url(url);

        if should_pass {
            assert!(result.is_ok(), "{}: {}", description, url);
        } else {
            assert!(result.is_err(), "{}: {}", description, url);
            if let Err(HyperliquidError::NotHyperliquidHost(host)) = result {
                assert!(!host.contains("hyperliquid"));
            }
        }
    }

    /// Test cases for malformed JSON handling
    #[rstest]
    #[case("{not json", "unclosed brace")]
    #[case("[1,2,", "unclosed array")]
    #[case("null", "null value")]
    #[case("", "empty string")]
    fn test_malformed_json_handling(#[case] invalid_json: &str, #[case] description: &str) {
        let codec = HyperliquidCodec::new();
        let result = codec.decode_text(invalid_json);

        assert!(
            result.is_err(),
            "Should fail for {}: {}",
            description,
            invalid_json
        );
        matches!(result.unwrap_err(), HyperliquidError::MalformedJson(_));
    }

    /// Test basic codec functionality
    #[test]
    fn test_codec_name_is_stable() {
        assert_eq!(HyperliquidCodec::new().name(), "hyperliquid");
    }

    /// Test ping message encoding
    #[test]
    fn test_encode_ping_message() {
        let codec = HyperliquidCodec::new();
        let frames = codec.encode(&WsOutbound::Ping).unwrap();

        assert_eq!(frames.len(), 1, "Ping should encode to single frame");
        assert_eq!(frames[0], json!({"method": "ping"}));
    }

    /// Test subscription encoding with various channel types
    #[rstest]
    #[case("trades", Some("BTC"), None, "basic trades subscription")]
    #[case("l2Book", Some("ETH"), None, "order book subscription")]
    #[case("bbo", Some("SOL"), None, "best bid/offer subscription")]
    #[case("userFills", None, Some(json!({"user": "0x123"})), "user fills with params")]
    fn test_encode_subscription(
        #[case] channel: &str,
        #[case] symbol: Option<&str>,
        #[case] params: Option<Value>,
        #[case] description: &str,
    ) {
        let codec = HyperliquidCodec::new();
        let sub_arg = SubArg {
            channel: channel.to_string(),
            symbol: symbol.map(|s| s.to_string()),
            params,
        };

        let result = codec
            .encode(&WsOutbound::Subscribe {
                args: vec![sub_arg],
                id: None,
            })
            .unwrap();

        assert_eq!(
            result.len(),
            1,
            "{}: should encode to single frame",
            description
        );

        let frame = &result[0];
        assert_eq!(frame["method"], "subscribe");
        assert_eq!(frame["subscription"]["type"], channel);

        // Verify symbol mapping for public channels
        if let Some(sym) = symbol {
            match channel {
                "l2Book" | "trades" | "bbo" | "candle" | "activeAssetCtx" | "activeAssetData" => {
                    assert_eq!(frame["subscription"]["coin"], sym);
                }
                _ => {
                    // Private channels don't use "coin" field
                }
            }
        }
    }

    /// Test unrecognized frame handling
    #[rstest]
    #[case(r#"{"channel": "unknown", "data": {}}"#, "unknown channel")]
    #[case(r#"{"method": "unknown"}"#, "unknown method")]
    #[case(r#"{"unrelated": "data"}"#, "unrelated JSON structure")]
    fn test_unrecognized_frames(#[case] json_text: &str, #[case] description: &str) {
        let codec = HyperliquidCodec::new();
        let result = codec.decode_text(json_text);

        assert!(
            result.is_err(),
            "Should reject {}: {}",
            description,
            json_text
        );
        matches!(result.unwrap_err(), HyperliquidError::UnrecognizedFrame);
    }

    /// Test binary frame handling (should be unsupported)
    #[test]
    fn test_binary_frames_unsupported() {
        let codec = HyperliquidCodec::new();
        let result = codec.decode_binary(&[0x01, 0x02, 0x03]);

        assert!(result.is_err());
        matches!(result.unwrap_err(), HyperliquidError::Unsupported(_));
    }
}
