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

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use futures_util::future::BoxFuture;
use hypersdk::hypercore::{BatchOrder, OrderTypePlacement, TimeInForce, api::Action};
use nautilus_common::live::get_runtime;
use nautilus_live::task::TaskGroup;
use tokio::{
    sync::{Mutex, OwnedSemaphorePermit, Semaphore, mpsc, oneshot},
    time,
};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{consts::HYPERLIQUID_WS_POST_INFLIGHT_MAX, enums::HyperliquidInfoRequestType},
    http::{
        error::{Error, Result},
        models::{HyperliquidFills, HyperliquidL2Book, HyperliquidOrderStatus},
    },
    websocket::messages::{HyperliquidWsRequest, PostRequest, PostResponse},
};

#[derive(Debug)]
struct Waiter {
    tx: oneshot::Sender<PostResponse>,
    cancellation_token: CancellationToken,
    // When this is dropped, the permit is released, shrinking inflight
    _permit: OwnedSemaphorePermit,
}

#[derive(Debug)]
pub struct PostRouter {
    inner: Mutex<AHashMap<u64, Waiter>>,
    inflight: Arc<Semaphore>, // hard cap per HL docs (e.g., 100)
}

impl Default for PostRouter {
    fn default() -> Self {
        Self {
            inner: Mutex::new(AHashMap::new()),
            inflight: Arc::new(Semaphore::new(HYPERLIQUID_WS_POST_INFLIGHT_MAX)),
        }
    }
}

impl PostRouter {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub(super) fn with_inflight(inflight: Arc<Semaphore>) -> Arc<Self> {
        Arc::new(Self {
            inner: Mutex::new(AHashMap::new()),
            inflight,
        })
    }

    /// Registers interest in a post id, enforcing inflight cap.
    pub async fn register(&self, id: u64) -> Result<oneshot::Receiver<PostResponse>> {
        self.register_waiter(id, &CancellationToken::new()).await
    }

    pub(super) async fn register_with_cancellation(
        self: &Arc<Self>,
        id: u64,
        cancellation_token: &CancellationToken,
    ) -> Result<oneshot::Receiver<PostResponse>> {
        let rx = self.register_waiter(id, cancellation_token).await?;
        let post_router = Arc::clone(self);
        let cancellation_token = cancellation_token.clone();
        get_runtime().spawn(async move {
            cancellation_token.cancelled().await;
            post_router
                .cancel_registration(id, &cancellation_token)
                .await;
        });

        Ok(rx)
    }

    async fn register_waiter(
        &self,
        id: u64,
        cancellation_token: &CancellationToken,
    ) -> Result<oneshot::Receiver<PostResponse>> {
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
                cancellation_token: cancellation_token.clone(),
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
            waiter.cancellation_token.cancel();
            if waiter.tx.send(resp).is_err() {
                log::warn!("Post waiter dropped before delivery: id={id}");
            }
            // waiter drops here → permit released
        } else {
            log::warn!("Post response with unknown id (late/duplicate?): id={id}");
        }
    }

    /// Cancel a pending id (e.g., timeout); quietly succeed if id wasn't present.
    pub async fn cancel(&self, id: u64) {
        let waiter = self.inner.lock().await.remove(&id);
        if let Some(waiter) = waiter {
            waiter.cancellation_token.cancel();
        }
        // Waiter (and its permit) drop here if it existed
    }

    pub(super) async fn cancel_registration(
        &self,
        id: u64,
        cancellation_token: &CancellationToken,
    ) {
        let waiter = {
            let mut map = self.inner.lock().await;
            if map
                .get(&id)
                .is_some_and(|waiter| &waiter.cancellation_token == cancellation_token)
            {
                map.remove(&id)
            } else {
                None
            }
        };

        if let Some(waiter) = waiter {
            waiter.cancellation_token.cancel();
        }
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
    _tasks: TaskGroup,
}

impl PostBatcher {
    /// Spawns two lane tasks that batch-send scheduled posts via `send_fn`.
    ///
    /// # Panics
    ///
    /// Panics if the new task group rejects either initial lane task.
    pub fn new<F>(send_fn: F) -> Self
    where
        F: Send + 'static + Clone + FnMut(HyperliquidWsRequest) -> BoxFuture<'static, Result<()>>,
    {
        let (tx_alo, rx_alo) = mpsc::channel::<ScheduledPost>(1024);
        let (tx_normal, rx_normal) = mpsc::channel::<ScheduledPost>(4096);
        let tasks = TaskGroup::new();

        // ALO lane: batchy tick, low jitter
        tasks
            .spawn(Self::run_lane(
                "ALO",
                rx_alo,
                Duration::from_millis(100),
                send_fn.clone(),
            ))
            .expect("new post batcher accepts ALO lane task");

        // NORMAL lane: faster tick; adjust as needed
        tasks
            .spawn(Self::run_lane(
                "NORMAL",
                rx_normal,
                Duration::from_millis(50),
                send_fn,
            ))
            .expect("new post batcher accepts normal lane task");

        Self {
            tx_alo,
            tx_normal,
            _tasks: tasks,
        }
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
                            log::error!("Failed to send post: lane={lane_name}, id={}, {e}", item.id);
                        }
                    }
                }
            }
        }
        log::debug!("Post lane terminated: lane={lane_name}");
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

// Classifies an action into its submission lane
pub fn lane_for_action(action: &Action) -> PostLane {
    match action {
        Action::Order(BatchOrder { orders, .. }) => {
            if orders.is_empty() {
                return PostLane::Normal;
            }
            let all_alo = orders.iter().all(|o| {
                matches!(
                    o.order_type,
                    OrderTypePlacement::Limit {
                        tif: TimeInForce::Alo
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

pub fn info_l2_book(coin: &str) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type": HyperliquidInfoRequestType::L2Book.as_str(), "coin": coin}),
    }
}

pub fn info_all_mids() -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type": HyperliquidInfoRequestType::AllMids.as_str()}),
    }
}

pub fn info_order_status(user: &str, oid: u64) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type": HyperliquidInfoRequestType::OrderStatus.as_str(), "user": user, "oid": oid}),
    }
}

pub fn info_open_orders(user: &str, frontend: Option<bool>) -> PostRequest {
    let mut body =
        serde_json::json!({"type": HyperliquidInfoRequestType::OpenOrders.as_str(), "user": user});

    if let Some(fe) = frontend {
        body["frontend"] = serde_json::json!(fe);
    }
    PostRequest::Info { payload: body }
}

pub fn info_user_fills(user: &str, aggregate_by_time: Option<bool>) -> PostRequest {
    let mut body =
        serde_json::json!({"type": HyperliquidInfoRequestType::UserFills.as_str(), "user": user});

    if let Some(agg) = aggregate_by_time {
        body["aggregateByTime"] = serde_json::json!(agg);
    }
    PostRequest::Info { payload: body }
}

pub fn info_user_rate_limit(user: &str) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type": HyperliquidInfoRequestType::UserRateLimit.as_str(), "user": user}),
    }
}

pub fn info_candle(coin: &str, interval: &str) -> PostRequest {
    PostRequest::Info {
        payload: serde_json::json!({"type": HyperliquidInfoRequestType::Candle.as_str(), "coin": coin, "interval": interval}),
    }
}

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

#[derive(Clone, Debug)]
pub struct WsSender {
    inner: mpsc::Sender<HyperliquidWsRequest>,
}

impl WsSender {
    pub fn new(tx: mpsc::Sender<HyperliquidWsRequest>) -> Self {
        Self { inner: tx }
    }

    pub async fn send(&self, req: HyperliquidWsRequest) -> Result<()> {
        self.inner
            .send(req)
            .await
            .map_err(|_| Error::transport("WebSocket sender closed"))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use hypersdk::hypercore::{Cloid, OrderGrouping, OrderRequest};
    use nautilus_common::{live::get_runtime, testing::wait_until_async};
    use rstest::rstest;
    use rust_decimal_macros::dec;
    use tokio::{
        sync::{Mutex as AsyncMutex, oneshot},
        time::{Duration, timeout},
    };

    use super::*;
    use crate::{
        common::consts::HYPERLIQUID_WS_POST_INFLIGHT_MAX,
        websocket::messages::{HyperliquidWsRequest, PostResponsePayload},
    };

    struct DropCounter(Arc<AtomicUsize>);

    impl Drop for DropCounter {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::Relaxed);
        }
    }

    fn mk_limit_alo(asset: usize) -> OrderRequest {
        OrderRequest {
            asset,
            is_buy: true,
            limit_px: dec!(1),
            sz: dec!(1),
            reduce_only: false,
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Alo,
            },
            cloid: Cloid::ZERO,
        }
    }

    fn mk_limit_gtc(asset: usize) -> OrderRequest {
        OrderRequest {
            order_type: OrderTypePlacement::Limit {
                tif: TimeInForce::Gtc,
            },
            ..mk_limit_alo(asset)
        }
    }

    #[tokio::test]
    async fn test_ws_sender_forwards_and_reports_closed_channel() {
        let (tx, mut rx) = mpsc::channel(1);
        let sender = WsSender::new(tx);

        sender.send(HyperliquidWsRequest::Ping).await.unwrap();
        assert!(matches!(rx.recv().await, Some(HyperliquidWsRequest::Ping)));

        drop(rx);
        let error = sender.send(HyperliquidWsRequest::Ping).await.unwrap_err();
        assert_eq!(
            error.to_string(),
            "transport error: WebSocket sender closed"
        );
    }

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
    #[tokio::test]
    async fn complete_cancels_registration_cleanup_and_allows_reregister() {
        let router = PostRouter::new();
        let id = 8;
        let cancellation_token = CancellationToken::new();
        let rx = router
            .register_with_cancellation(id, &cancellation_token)
            .await
            .unwrap();

        router
            .complete(PostResponse {
                id,
                response: PostResponsePayload::Info {
                    payload: serde_json::json!({"status": "ok"}),
                },
            })
            .await;
        let response = rx.await.unwrap();

        assert_eq!(response.id, id);
        assert!(matches!(
            response.response,
            PostResponsePayload::Info { .. }
        ));
        assert!(cancellation_token.is_cancelled());
        router
            .register(id)
            .await
            .expect("id should be reusable after completion");
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn batcher_sends_on_tick() {
        // Capture sent ids to prove dispatch happened.
        let sent: Arc<AsyncMutex<Vec<u64>>> = Arc::new(AsyncMutex::new(Vec::new()));
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
                    request: info_all_mids(),
                    lane: PostLane::Normal,
                })
                .await
                .unwrap();
        }

        // Wait for all 5 posts to be sent
        let sent_check = sent.clone();
        wait_until_async(
            || {
                let sent_inner = sent_check.clone();
                async move { sent_inner.lock().await.len() == 5 }
            },
            Duration::from_secs(2),
        )
        .await;

        let actual = sent.lock().await.clone();
        assert_eq!(actual, vec![1, 2, 3, 4, 5]);
    }

    #[rstest]
    #[tokio::test(flavor = "multi_thread")]
    async fn inflight_cap_blocks_then_unblocks() {
        let router = PostRouter::new();

        // Fill the inflight capacity.
        let mut rxs = Vec::with_capacity(HYPERLIQUID_WS_POST_INFLIGHT_MAX);
        for i in 0..HYPERLIQUID_WS_POST_INFLIGHT_MAX {
            let rx = router.register(i as u64).await.unwrap();
            rxs.push(rx); // keep waiters alive
        }

        // Next register should block until a permit is freed.
        let router2 = Arc::clone(&router);
        let (entered_tx, entered_rx) = oneshot::channel::<()>();
        let (done_tx, done_rx) = oneshot::channel::<()>();
        let (check_tx, check_rx) = oneshot::channel::<()>(); // separate channel for checking

        get_runtime().spawn(async move {
            let _ = entered_tx.send(());
            let _rx = router2.register(9_999_999).await.unwrap();
            let _ = done_tx.send(());
        });

        // Confirm the task is trying to register…
        entered_rx.await.unwrap();

        // …and that it doesn't complete yet (still blocked on permit).
        get_runtime().spawn(async move {
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

    #[rstest(
        orders, expected,
        case::all_alo(vec![mk_limit_alo(0), mk_limit_alo(1)], PostLane::Alo),
        case::mixed_alo_gtc(vec![mk_limit_alo(0), mk_limit_gtc(1)], PostLane::Normal),
        case::all_gtc(vec![mk_limit_gtc(0), mk_limit_gtc(1)], PostLane::Normal),
        case::empty(vec![], PostLane::Normal),
    )]
    fn lane_classifier_cases(orders: Vec<OrderRequest>, expected: PostLane) {
        let action = Action::Order(BatchOrder {
            orders,
            grouping: OrderGrouping::Na,
            builder: None,
        });
        assert_eq!(lane_for_action(&action), expected);
    }

    #[tokio::test]
    async fn test_batcher_drop_aborts_lane_tasks() {
        let started = Arc::new(AtomicUsize::new(0));
        let dropped = Arc::new(AtomicUsize::new(0));
        let started_send = Arc::clone(&started);
        let dropped_send = Arc::clone(&dropped);
        let send_fn = move |_req: HyperliquidWsRequest| -> BoxFuture<'static, Result<()>> {
            let started = Arc::clone(&started_send);
            let dropped = Arc::clone(&dropped_send);
            Box::pin(async move {
                let _drop_counter = DropCounter(dropped);
                started.fetch_add(1, Ordering::Relaxed);
                std::future::pending::<Result<()>>().await
            })
        };
        let batcher = PostBatcher::new(send_fn);

        for (id, lane) in [(1, PostLane::Alo), (2, PostLane::Normal)] {
            batcher
                .enqueue(ScheduledPost {
                    id,
                    request: info_all_mids(),
                    lane,
                })
                .await
                .unwrap();
        }
        let started_check = Arc::clone(&started);
        wait_until_async(
            || {
                let started = Arc::clone(&started_check);
                async move { started.load(Ordering::Relaxed) == 2 }
            },
            Duration::from_secs(1),
        )
        .await;

        drop(batcher);

        let dropped_check = Arc::clone(&dropped);
        wait_until_async(
            || {
                let dropped = Arc::clone(&dropped_check);
                async move { dropped.load(Ordering::Relaxed) == 2 }
            },
            Duration::from_secs(1),
        )
        .await;
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }
}
