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
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use derive_builder::Builder;
use futures_util::future::BoxFuture;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    time,
};

use crate::{
    common::consts::INFLIGHT_MAX,
    http::{
        error::{Error, Result},
        models::{HyperliquidFills, HyperliquidL2Book, HyperliquidOrderStatus},
    },
    websocket::messages::{
        ActionRequest, CancelByCloidRequest, CancelRequest, HyperliquidWsRequest, ModifyRequest,
        OrderRequest, OrderTypeRequest, PostRequest, PostResponse, TimeInForceRequest, TpSlRequest,
    },
};

// -------------------------------------------------------------------------------------------------
// Correlation router for "channel":"post" → correlate by id
//  - Enforces inflight cap using OwnedSemaphorePermit stored per waiter
// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
struct Waiter {
    tx: oneshot::Sender<PostResponse>,
    // When this is dropped, the permit is released, shrinking inflight
    _permit: OwnedSemaphorePermit,
}

#[derive(Debug)]
pub struct PostRouter {
    inner: Mutex<HashMap<u64, Waiter>>,
    inflight: Arc<Semaphore>, // hard cap per HL docs (e.g., 100)
}

impl Default for PostRouter {
    fn default() -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            inflight: Arc::new(Semaphore::new(INFLIGHT_MAX)),
        }
    }
}

impl PostRouter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Registers interest in a post id, enforcing inflight cap.
    pub async fn register(&self, id: u64) -> Result<oneshot::Receiver<PostResponse>> {
        // Acquire and retain a permit per inflight call
        let permit = self
            .inflight
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| Error::transport("post router semaphore closed"))?;

        let (tx, rx) = oneshot::channel::<PostResponse>();
        let mut map = self.inner.lock().await;
        if map.contains_key(&id) {
            return Err(Error::transport(format!("post id {id} already registered")));
        }
        map.insert(
            id,
            Waiter {
                tx,
                _permit: permit,
            },
        );
        Ok(rx)
    }

    /// Completes a waiting caller when a response arrives (releases inflight via Waiter drop).
    pub async fn complete(&self, resp: PostResponse) {
        let id = resp.id;
        let waiter = {
            let mut map = self.inner.lock().await;
            map.remove(&id)
        };
        if let Some(waiter) = waiter {
            if waiter.tx.send(resp).is_err() {
                tracing::warn!(id, "post waiter dropped before delivery");
            }
            // waiter drops here → permit released
        } else {
            tracing::warn!(id, "post response with unknown id (late/duplicate?)");
        }
    }

    /// Cancel a pending id (e.g., timeout); quietly succeed if id wasn't present.
    pub async fn cancel(&self, id: u64) {
        let _ = {
            let mut map = self.inner.lock().await;
            map.remove(&id)
        };
        // Waiter (and its permit) drop here if it existed
    }

    /// Await a response with timeout. On timeout or closed channel, cancels the id.
    pub async fn await_with_timeout(
        &self,
        id: u64,
        rx: oneshot::Receiver<PostResponse>,
        timeout: Duration,
    ) -> Result<PostResponse> {
        match time::timeout(timeout, rx).await {
            Ok(Ok(resp)) => Ok(resp),
            Ok(Err(_closed)) => {
                self.cancel(id).await;
                Err(Error::transport("post response channel closed"))
            }
            Err(_elapsed) => {
                self.cancel(id).await;
                Err(Error::Timeout)
            }
        }
    }
}

// -------------------------------------------------------------------------------------------------
// ID generation
// -------------------------------------------------------------------------------------------------

#[derive(Debug)]
pub struct PostIds(AtomicU64);

impl PostIds {
    pub fn new(start: u64) -> Self {
        Self(AtomicU64::new(start))
    }
    pub fn next(&self) -> u64 {
        self.0.fetch_add(1, Ordering::Relaxed)
    }
}

// -------------------------------------------------------------------------------------------------
// Lanes & batcher (scaffold). You can expand policy later.
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PostLane {
    Alo,    // Post-only orders
    Normal, // IOC/GTC + info + anything else
}

#[derive(Debug)]
pub struct ScheduledPost {
    pub id: u64,
    pub request: PostRequest,
    pub lane: PostLane,
}

#[derive(Debug)]
pub struct PostBatcher {
    tx_alo: mpsc::Sender<ScheduledPost>,
    tx_normal: mpsc::Sender<ScheduledPost>,
}

impl PostBatcher {
    /// Spawns two lane tasks that batch-send scheduled posts via `send_fn`.
    pub fn new<F>(send_fn: F) -> Self
    where
        F: Send + 'static + Clone + FnMut(HyperliquidWsRequest) -> BoxFuture<'static, Result<()>>,
    {
        let (tx_alo, rx_alo) = mpsc::channel::<ScheduledPost>(1024);
        let (tx_normal, rx_normal) = mpsc::channel::<ScheduledPost>(4096);

        // ALO lane: batchy tick, low jitter
        tokio::spawn(Self::run_lane(
            "ALO",
            rx_alo,
            Duration::from_millis(100),
            send_fn.clone(),
        ));

        // NORMAL lane: faster tick; adjust as needed
        tokio::spawn(Self::run_lane(
            "NORMAL",
            rx_normal,
            Duration::from_millis(50),
            send_fn,
        ));

        Self { tx_alo, tx_normal }
    }

    async fn run_lane<F>(
        lane_name: &'static str,
        mut rx: mpsc::Receiver<ScheduledPost>,
        tick: Duration,
        mut send_fn: F,
    ) where
        F: Send + 'static + FnMut(HyperliquidWsRequest) -> BoxFuture<'static, Result<()>>,
    {
        let mut pend: Vec<ScheduledPost> = Vec::with_capacity(128);
        let mut interval = time::interval(tick);
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Delay);

        loop {
            tokio::select! {
                maybe_item = rx.recv() => {
                    match maybe_item {
                        Some(item) => pend.push(item),
                        None => break, // sender dropped → terminate lane task
                    }
                }
                _ = interval.tick() => {
                    if pend.is_empty() { continue; }
                    let to_send = std::mem::take(&mut pend);
                    for item in to_send {
                        let req = HyperliquidWsRequest::Post { id: item.id, request: item.request.clone() };
                        if let Err(e) = send_fn(req).await {
                            tracing::error!(lane=%lane_name, id=%item.id, "failed to send post: {e}");
                        }
                    }
                }
            }
        }
        tracing::info!(lane=%lane_name, "post lane terminated");
    }

    pub async fn enqueue(&self, item: ScheduledPost) -> Result<()> {
        match item.lane {
            PostLane::Alo => self
                .tx_alo
                .send(item)
                .await
                .map_err(|_| Error::transport("ALO lane closed")),
            PostLane::Normal => self
                .tx_normal
                .send(item)
                .await
                .map_err(|_| Error::transport("NORMAL lane closed")),
        }
    }
}

// Helpers to classify lane from an action
pub fn lane_for_action(action: &ActionRequest) -> PostLane {
    match action {
        ActionRequest::Order { orders, .. } => {
            if orders.is_empty() {
                return PostLane::Normal;
            }
            let all_alo = orders.iter().all(|o| {
                matches!(
                    o.t,
                    OrderTypeRequest::Limit {
                        tif: TimeInForceRequest::Alo
                    }
                )
            });
            if all_alo {
                PostLane::Alo
            } else {
                PostLane::Normal
            }
        }
        _ => PostLane::Normal,
    }
}

// -------------------------------------------------------------------------------------------------
// Typed builders (produce ActionRequest), plus Info request helpers.
// -------------------------------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, Default)]
pub enum Grouping {
    #[default]
    Na,
    NormalTpsl,
    PositionTpsl,
}
impl Grouping {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Na => "na",
            Self::NormalTpsl => "normalTpsl",
            Self::PositionTpsl => "positionTpsl",
        }
    }
}

/// Parameters for creating a limit order
#[derive(Debug, Clone, Builder)]
pub struct LimitOrderParams {
    pub asset: u32,
    pub is_buy: bool,
    pub px: String,
    pub sz: String,
    pub reduce_only: bool,
    pub tif: TimeInForceRequest,
    pub cloid: Option<String>,
}

/// Parameters for creating a trigger order
#[derive(Debug, Clone, Builder)]
pub struct TriggerOrderParams {
    pub asset: u32,
    pub is_buy: bool,
    pub px: String,
    pub sz: String,
    pub reduce_only: bool,
    pub is_market: bool,
    pub trigger_px: String,
    pub tpsl: TpSlRequest,
    pub cloid: Option<String>,
}

// ORDER builder (single or many)
#[derive(Debug, Default)]
pub struct OrderBuilder {
    orders: Vec<OrderRequest>,
    grouping: Grouping,
}

impl OrderBuilder {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn grouping(mut self, g: Grouping) -> Self {
        self.grouping = g;
        self
    }

    /// Create a limit order with individual parameters (legacy method)
    #[allow(clippy::too_many_arguments)]
    pub fn push_limit(
        self,
        asset: u32,
        is_buy: bool,
        px: impl ToString,
        sz: impl ToString,
        reduce_only: bool,
        tif: TimeInForceRequest,
        cloid: Option<String>,
    ) -> Self {
        let params = LimitOrderParams {
            asset,
            is_buy,
            px: px.to_string(),
            sz: sz.to_string(),
            reduce_only,
            tif,
            cloid,
        };
        self.push_limit_order(params)
    }

    /// Create a limit order using parameters struct
    pub fn push_limit_order(mut self, params: LimitOrderParams) -> Self {
        self.orders.push(OrderRequest {
            a: params.asset,
            b: params.is_buy,
            p: params.px,
            s: params.sz,
            r: params.reduce_only,
            t: OrderTypeRequest::Limit { tif: params.tif },
            c: params.cloid,
        });
        self
    }

    /// Create a trigger order with individual parameters (legacy method)
    #[allow(clippy::too_many_arguments)]
    pub fn push_trigger(
        self,
        asset: u32,
        is_buy: bool,
        px: impl ToString,
        sz: impl ToString,
        reduce_only: bool,
        is_market: bool,
        trigger_px: impl ToString,
        tpsl: TpSlRequest,
        cloid: Option<String>,
    ) -> Self {
        let params = TriggerOrderParams {
            asset,
            is_buy,
            px: px.to_string(),
            sz: sz.to_string(),
            reduce_only,
            is_market,
            trigger_px: trigger_px.to_string(),
            tpsl,
            cloid,
        };
        self.push_trigger_order(params)
    }

    /// Create a trigger order using parameters struct
    pub fn push_trigger_order(mut self, params: TriggerOrderParams) -> Self {
        self.orders.push(OrderRequest {
            a: params.asset,
            b: params.is_buy,
            p: params.px,
            s: params.sz,
            r: params.reduce_only,
            t: OrderTypeRequest::Trigger {
                is_market: params.is_market,
                trigger_px: params.trigger_px,
                tpsl: params.tpsl,
            },
            c: params.cloid,
        });
        self
    }
    pub fn build(self) -> ActionRequest {
        ActionRequest::Order {
            orders: self.orders,
            grouping: self.grouping.as_str().to_string(),
        }
    }

    /// Create a single limit order action directly (convenience method)
    ///
    /// # Example
    /// ```ignore
    /// let action = OrderBuilder::single_limit_order(
    ///     LimitOrderParamsBuilder::default()
    ///         .asset(0)
    ///         .is_buy(true)
    ///         .px("40000.0")
    ///         .sz("0.01")
    ///         .reduce_only(false)
    ///         .tif(TimeInForceRequest::Gtc)
    ///         .build()
    ///         .unwrap()
    /// );
    /// ```
    pub fn single_limit_order(params: LimitOrderParams) -> ActionRequest {
        Self::new().push_limit_order(params).build()
    }

    /// Create a single trigger order action directly (convenience method)
    ///
    /// # Example
    /// ```ignore
    /// let action = OrderBuilder::single_trigger_order(
    ///     TriggerOrderParamsBuilder::default()
    ///         .asset(0)
    ///         .is_buy(false)
    ///         .px("39000.0")
    ///         .sz("0.01")
    ///         .reduce_only(false)
    ///         .is_market(true)
    ///         .trigger_px("39500.0")
    ///         .tpsl(TpSlRequest::Sl)
    ///         .build()
    ///         .unwrap()
    /// );
    /// ```
    pub fn single_trigger_order(params: TriggerOrderParams) -> ActionRequest {
        Self::new().push_trigger_order(params).build()
    }
}

pub fn cancel_many(cancels: Vec<(u32, u64)>) -> ActionRequest {
    ActionRequest::Cancel {
        cancels: cancels
            .into_iter()
            .map(|(a, o)| CancelRequest { a, o })
            .collect(),
    }
}
pub fn cancel_by_cloid(asset: u32, cloid: impl Into<String>) -> ActionRequest {
    ActionRequest::CancelByCloid {
        cancels: vec![CancelByCloidRequest {
            asset,
            cloid: cloid.into(),
        }],
    }
}
pub fn modify(oid: u64, new_order: OrderRequest) -> ActionRequest {
    ActionRequest::Modify {
        modifies: vec![ModifyRequest {
            oid,
            order: new_order,
        }],
    }
}

// Info wrappers (bodies go under PostRequest::Info{ payload })
pub fn info_l2_book(coin: &str) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type":"l2Book","coin":coin}),
    }
}
pub fn info_all_mids() -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type":"allMids"}),
    }
}
pub fn info_order_status(user: &str, oid: u64) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type":"orderStatus","user":user,"oid":oid}),
    }
}
pub fn info_open_orders(user: &str, frontend: Option<bool>) -> PostRequest {
    let mut body = serde_json::json!({"type":"openOrders","user":user});
    if let Some(fe) = frontend {
        body["frontend"] = serde_json::json!(fe);
    }
    PostRequest::Info { payload: body }
}
pub fn info_user_fills(user: &str, aggregate_by_time: Option<bool>) -> PostRequest {
    let mut body = serde_json::json!({"type":"userFills","user":user});
    if let Some(agg) = aggregate_by_time {
        body["aggregateByTime"] = serde_json::json!(agg);
    }
    PostRequest::Info { payload: body }
}
pub fn info_user_rate_limit(user: &str) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type":"userRateLimit","user":user}),
    }
}
pub fn info_candle(coin: &str, interval: &str) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type":"candle","coin":coin,"interval":interval}),
    }
}

// -------------------------------------------------------------------------------------------------
// Minimal response helpers
// -------------------------------------------------------------------------------------------------

pub fn parse_l2_book(payload: &serde_json::Value) -> Result<HyperliquidL2Book> {
    serde_json::from_value(payload.clone()).map_err(Error::Serde)
}
pub fn parse_user_fills(payload: &serde_json::Value) -> Result<HyperliquidFills> {
    serde_json::from_value(payload.clone()).map_err(Error::Serde)
}
pub fn parse_order_status(payload: &serde_json::Value) -> Result<HyperliquidOrderStatus> {
    serde_json::from_value(payload.clone()).map_err(Error::Serde)
}

/// Heuristic classification for action responses.
#[derive(Debug)]
pub enum ActionOutcome<'a> {
    Resting {
        oid: u64,
    },
    Filled {
        total_sz: &'a str,
        avg_px: &'a str,
        oid: Option<u64>,
    },
    Error {
        msg: &'a str,
    },
    Unknown(&'a serde_json::Value),
}
pub fn classify_action_payload(payload: &serde_json::Value) -> ActionOutcome<'_> {
    if let Some(oid) = payload.get("oid").and_then(|v| v.as_u64()) {
        if let (Some(total_sz), Some(avg_px)) = (
            payload.get("totalSz").and_then(|v| v.as_str()),
            payload.get("avgPx").and_then(|v| v.as_str()),
        ) {
            return ActionOutcome::Filled {
                total_sz,
                avg_px,
                oid: Some(oid),
            };
        }
        return ActionOutcome::Resting { oid };
    }
    if let (Some(total_sz), Some(avg_px)) = (
        payload.get("totalSz").and_then(|v| v.as_str()),
        payload.get("avgPx").and_then(|v| v.as_str()),
    ) {
        return ActionOutcome::Filled {
            total_sz,
            avg_px,
            oid: None,
        };
    }
    if let Some(msg) = payload
        .get("error")
        .and_then(|v| v.as_str())
        .or_else(|| payload.get("message").and_then(|v| v.as_str()))
    {
        return ActionOutcome::Error { msg };
    }
    ActionOutcome::Unknown(payload)
}

// -------------------------------------------------------------------------------------------------
// Glue helpers used by the client (wired in client.rs)
// -------------------------------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct WsSender {
    inner: Arc<tokio::sync::Mutex<mpsc::Sender<HyperliquidWsRequest>>>,
}

impl WsSender {
    pub fn new(tx: mpsc::Sender<HyperliquidWsRequest>) -> Self {
        Self {
            inner: Arc::new(tokio::sync::Mutex::new(tx)),
        }
    }

    pub async fn send(&self, req: HyperliquidWsRequest) -> Result<()> {
        let sender = self.inner.lock().await;
        sender
            .send(req)
            .await
            .map_err(|_| Error::transport("WebSocket sender closed"))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use tokio::{
        sync::oneshot,
        time::{Duration, sleep, timeout},
    };

    use super::*;
    use crate::{
        common::consts::INFLIGHT_MAX,
        websocket::messages::{
            ActionRequest, CancelByCloidRequest, CancelRequest, HyperliquidWsRequest, OrderRequest,
            OrderRequestBuilder, OrderTypeRequest, TimeInForceRequest,
        },
    };

    // --- helpers -------------------------------------------------------------------------------

    fn mk_limit_alo(asset: u32) -> OrderRequest {
        OrderRequest {
            a: asset,
            b: true,
            p: "1".to_string(),
            s: "1".to_string(),
            r: false,
            t: OrderTypeRequest::Limit {
                tif: TimeInForceRequest::Alo,
            },
            c: None,
        }
    }

    fn mk_limit_gtc(asset: u32) -> OrderRequest {
        OrderRequest {
            a: asset,
            b: true,
            p: "1".to_string(),
            s: "1".to_string(),
            r: false,
            t: OrderTypeRequest::Limit {
                // any non-ALO TIF keeps it in the Normal lane
                tif: TimeInForceRequest::Gtc,
            },
            c: None,
        }
    }

    // --- PostRouter ---------------------------------------------------------------------------

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn register_duplicate_id_errors() {
        let router = PostRouter::new();
        let _rx = router.register(42).await.expect("first register OK");

        let err = router.register(42).await.expect_err("duplicate must error");
        let msg = err.to_string().to_lowercase();
        assert!(
            msg.contains("already") || msg.contains("duplicate"),
            "unexpected error: {msg}"
        );
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn timeout_cancels_and_allows_reregister() {
        let router = PostRouter::new();
        let id = 7;

        let rx = router.register(id).await.unwrap();
        // No complete() → ensure we time out and the waiter is removed.
        let err = router
            .await_with_timeout(id, rx, Duration::from_millis(25))
            .await
            .expect_err("should timeout");
        assert!(
            err.to_string().to_lowercase().contains("timeout")
                || err.to_string().to_lowercase().contains("closed"),
            "unexpected error kind: {err}"
        );

        // After timeout, id should be reusable (cancel dropped the waiter & released the permit).
        let _rx2 = router
            .register(id)
            .await
            .expect("id should be reusable after timeout cancel");
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn inflight_cap_blocks_then_unblocks() {
        let router = PostRouter::new();

        // Fill the inflight capacity.
        let mut rxs = Vec::with_capacity(INFLIGHT_MAX);
        for i in 0..INFLIGHT_MAX {
            let rx = router.register(i as u64).await.unwrap();
            rxs.push(rx); // keep waiters alive
        }

        // Next register should block until a permit is freed.
        let router2 = Arc::clone(&router);
        let (entered_tx, entered_rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = oneshot::channel::<()>();
        let (check_tx, check_rx) = oneshot::channel::<()>(); // separate channel for checking

        tokio::spawn(async move {
            let _ = entered_tx.send(());
            let _rx = router2.register(9_999_999).await.unwrap();
            let _ = done_tx.send(());
        });

        // Confirm the task is trying to register…
        entered_rx.await.unwrap();

        // …and that it doesn't complete yet (still blocked on permit).
        tokio::spawn(async move {
            if done_rx.await.is_ok() {
                let _ = check_tx.send(());
            }
        });

        assert!(
            timeout(Duration::from_millis(50), check_rx).await.is_err(),
            "should still be blocked while at cap"
        );

        // Free one permit by cancelling a waiter.
        router.cancel(0).await;

        // Wait for the blocked register to complete.
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // --- Lane classifier -----------------------------------------------------------------------

    #[rstest(
        orders, expected,
        case::all_alo(vec![mk_limit_alo(0), mk_limit_alo(1)], PostLane::Alo),
        case::mixed_alo_gtc(vec![mk_limit_alo(0), mk_limit_gtc(1)], PostLane::Normal),
        case::all_gtc(vec![mk_limit_gtc(0), mk_limit_gtc(1)], PostLane::Normal),
        case::empty(vec![], PostLane::Normal),
    )]
    fn lane_classifier_cases(orders: Vec<OrderRequest>, expected: PostLane) {
        let action = ActionRequest::Order {
            orders,
            grouping: "na".to_string(),
        };
        assert_eq!(lane_for_action(&action), expected);
    }

    // --- Builder Pattern Tests -----------------------------------------------------------------

    #[test]
    fn test_order_request_builder() {
        // Test OrderRequestBuilder derived from #[derive(Builder)]
        let order = OrderRequestBuilder::default()
            .a(0)
            .b(true)
            .p("40000.0".to_string())
            .s("0.01".to_string())
            .r(false)
            .t(OrderTypeRequest::Limit {
                tif: TimeInForceRequest::Gtc,
            })
            .c(Some("test-order-1".to_string()))
            .build()
            .expect("should build order");

        assert_eq!(order.a, 0);
        assert!(order.b);
        assert_eq!(order.p, "40000.0");
        assert_eq!(order.s, "0.01");
        assert!(!order.r);
        assert_eq!(order.c, Some("test-order-1".to_string()));
    }

    #[test]
    fn test_limit_order_params_builder() {
        // Test LimitOrderParamsBuilder
        let params = LimitOrderParamsBuilder::default()
            .asset(0)
            .is_buy(true)
            .px("40000.0".to_string())
            .sz("0.01".to_string())
            .reduce_only(false)
            .tif(TimeInForceRequest::Alo)
            .cloid(Some("test-limit-1".to_string()))
            .build()
            .expect("should build limit params");

        assert_eq!(params.asset, 0);
        assert!(params.is_buy);
        assert_eq!(params.px, "40000.0");
        assert_eq!(params.sz, "0.01");
        assert!(!params.reduce_only);
        assert_eq!(params.cloid, Some("test-limit-1".to_string()));
    }

    #[test]
    fn test_trigger_order_params_builder() {
        // Test TriggerOrderParamsBuilder
        let params = TriggerOrderParamsBuilder::default()
            .asset(1)
            .is_buy(false)
            .px("39000.0".to_string())
            .sz("0.02".to_string())
            .reduce_only(false)
            .is_market(true)
            .trigger_px("39500.0".to_string())
            .tpsl(TpSlRequest::Sl)
            .cloid(Some("test-trigger-1".to_string()))
            .build()
            .expect("should build trigger params");

        assert_eq!(params.asset, 1);
        assert!(!params.is_buy);
        assert_eq!(params.px, "39000.0");
        assert!(params.is_market);
        assert_eq!(params.trigger_px, "39500.0");
    }

    #[test]
    fn test_order_builder_single_limit_convenience() {
        // Test OrderBuilder::single_limit_order convenience method
        let params = LimitOrderParamsBuilder::default()
            .asset(0)
            .is_buy(true)
            .px("40000.0".to_string())
            .sz("0.01".to_string())
            .reduce_only(false)
            .tif(TimeInForceRequest::Gtc)
            .cloid(None)
            .build()
            .unwrap();

        let action = OrderBuilder::single_limit_order(params);

        match action {
            ActionRequest::Order { orders, grouping } => {
                assert_eq!(orders.len(), 1);
                assert_eq!(orders[0].a, 0);
                assert!(orders[0].b);
                assert_eq!(grouping, "na");
            }
            _ => panic!("Expected ActionRequest::Order variant"),
        }
    }

    #[test]
    fn test_order_builder_single_trigger_convenience() {
        // Test OrderBuilder::single_trigger_order convenience method
        let params = TriggerOrderParamsBuilder::default()
            .asset(1)
            .is_buy(false)
            .px("39000.0".to_string())
            .sz("0.02".to_string())
            .reduce_only(false)
            .is_market(true)
            .trigger_px("39500.0".to_string())
            .tpsl(TpSlRequest::Sl)
            .cloid(Some("sl-order".to_string()))
            .build()
            .unwrap();

        let action = OrderBuilder::single_trigger_order(params);

        match action {
            ActionRequest::Order { orders, grouping } => {
                assert_eq!(orders.len(), 1);
                assert_eq!(orders[0].a, 1);
                assert_eq!(orders[0].c, Some("sl-order".to_string()));
                assert_eq!(grouping, "na");
            }
            _ => panic!("Expected ActionRequest::Order variant"),
        }
    }

    #[test]
    fn test_order_builder_batch_orders() {
        // Test existing batch order functionality still works
        let params1 = LimitOrderParams {
            asset: 0,
            is_buy: true,
            px: "40000.0".to_string(),
            sz: "0.01".to_string(),
            reduce_only: false,
            tif: TimeInForceRequest::Gtc,
            cloid: Some("order-1".to_string()),
        };

        let params2 = LimitOrderParams {
            asset: 1,
            is_buy: false,
            px: "2000.0".to_string(),
            sz: "0.5".to_string(),
            reduce_only: false,
            tif: TimeInForceRequest::Ioc,
            cloid: Some("order-2".to_string()),
        };

        let action = OrderBuilder::new()
            .grouping(Grouping::NormalTpsl)
            .push_limit_order(params1)
            .push_limit_order(params2)
            .build();

        match action {
            ActionRequest::Order { orders, grouping } => {
                assert_eq!(orders.len(), 2);
                assert_eq!(orders[0].c, Some("order-1".to_string()));
                assert_eq!(orders[1].c, Some("order-2".to_string()));
                assert_eq!(grouping, "normalTpsl");
            }
            _ => panic!("Expected ActionRequest::Order variant"),
        }
    }

    #[test]
    fn test_action_request_constructors() {
        // Test ActionRequest::order() constructor
        let order1 = mk_limit_gtc(0);
        let order2 = mk_limit_gtc(1);
        let action = ActionRequest::order(vec![order1, order2], "na");

        match action {
            ActionRequest::Order { orders, grouping } => {
                assert_eq!(orders.len(), 2);
                assert_eq!(grouping, "na");
            }
            _ => panic!("Expected ActionRequest::Order variant"),
        }

        // Test ActionRequest::cancel() constructor
        let cancels = vec![CancelRequest { a: 0, o: 12345 }];
        let action = ActionRequest::cancel(cancels);
        assert!(matches!(action, ActionRequest::Cancel { .. }));

        // Test ActionRequest::cancel_by_cloid() constructor
        let cancels = vec![CancelByCloidRequest {
            asset: 0,
            cloid: "order-1".to_string(),
        }];
        let action = ActionRequest::cancel_by_cloid(cancels);
        assert!(matches!(action, ActionRequest::CancelByCloid { .. }));
    }

    // --- Batcher (tick flush path) --------------------------------------------------------------

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn batcher_sends_on_tick() {
        // Capture sent ids to prove dispatch happened.
        let sent: Arc<tokio::sync::Mutex<Vec<u64>>> = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let sent_closure = sent.clone();

        let send_fn = move |req: HyperliquidWsRequest| -> BoxFuture<'static, Result<()>> {
            let sent_inner = sent_closure.clone();
            Box::pin(async move {
                if let HyperliquidWsRequest::Post { id, .. } = req {
                    sent_inner.lock().await.push(id);
                }
                Ok(())
            })
        };

        let batcher = PostBatcher::new(send_fn);

        // Enqueue a handful of posts into the NORMAL lane; tick is ~50ms.
        for id in 1..=5u64 {
            batcher
                .enqueue(ScheduledPost {
                    id,
                    request: PostRequest::Info {
                        payload: serde_json::json!({"type":"allMids"}),
                    },
                    lane: PostLane::Normal,
                })
                .await
                .unwrap();
        }

        // Wait slightly past one tick to allow the lane to flush.
        sleep(Duration::from_millis(80)).await;

        let got = sent.lock().await.clone();
        assert_eq!(got.len(), 5, "expected 5 sends on first tick");
        assert_eq!(got, vec![1, 2, 3, 4, 5]);
    }
}
