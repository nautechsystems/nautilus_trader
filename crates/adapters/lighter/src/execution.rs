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

//! Live execution client for the Lighter adapter.
//!
//! This module hosts the [`LighterExecutionClient`] that wires the platform
//! execution engine to the Lighter L2 sequencer. Order submission,
//! cancellation, and modification use signed WebSocket trading transactions.
//! Reconciliation and report generation combine Lighter's account WebSocket
//! streams with the HTTP read endpoints.
//!
//! Auth-token rotation is owned by this execution client and refreshes the
//! private account-stream subscriptions on
//! [`crate::websocket::client::LighterWebSocketClient`].

use std::{
    collections::{BTreeMap, BTreeSet},
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    enums::LogColor,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    log_info,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    params::Params,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, ContingencyType, OmsType, OrderSide, OrderType, PositionSideSpecified},
    events::{OrderAccepted, OrderEventAny},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{
            LIGHTER_ERROR_CODE_INVALID_NONCE, LIGHTER_MAX_BATCH_TX,
            LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX, LIGHTER_VENUE,
        },
        credential::{Credential, scrub_auth},
        enums::{LighterAccountTier, LighterPositionMarginMode, LighterProductType, LighterTxType},
        rate_limit::{LighterTxRateLimiter, await_tx_quota, build_tx_rate_limiter, resolve_quota},
        symbol::{MarketRegistry, product_type_from_instrument_id},
        urls::lighter_chain_id,
    },
    config::LighterExecClientConfig,
    http::{
        client::{LIGHTER_REST_PAGE_SIZE, LighterHttpClient, LighterRawHttpClient},
        error::LighterHttpError,
        models::{LighterSendTxBatchRequest, LighterSendTxRequest},
        query::{
            LighterAccountActiveOrdersQuery, LighterAccountInactiveOrdersQuery,
            LighterSortDirection, LighterTradeSortBy, LighterTradesQuery,
        },
    },
    signing::{
        auth_token::{build_auth_token_for, fresh_k},
        nonce::NonceError,
        tx::{
            ApproveIntegratorTxInfo, CancelOrderTxInfo, CreateOrderTxInfo, L2TxAttributes,
            ModifyOrderTxInfo, OrderInfo, TxContext, TxInfoJson, UpdateLeverageTxInfo, sign_tx,
        },
    },
    websocket::{
        client::LighterWebSocketClient,
        dispatch::{
            LIGHTER_INSTRUMENT_CACHE, OrderIdentity, PendingSendTx, PendingSendTxKind,
            WsDispatchState, cache_instruments_for_reports, derive_market_order_price_ticks,
            evict_terminal_mappings, lookup_order_status_report, nautilus_to_lighter_order_type,
            nautilus_to_lighter_tif, order_expiry_for, parse_http_order_to_report, price_to_ticks,
            quantity_to_ticks, resolve_cloid, translate_fill_cloid, translate_order_cloid,
            unwrap_reports_or_warn,
        },
        messages::{
            AccountStream, ExecutionReport, LighterWsChannel, NautilusWsMessage,
            SendTxRejectionSource,
        },
        parse::{
            OpenFrameContext, ParsedOrderEvent, lighter_order_shape, parse_lighter_order_event,
            parse_lighter_order_filled, parse_lighter_trade_id, parse_ws_fill_report,
            parse_ws_order_status_report,
        },
    },
};

/// Default `expired_at` window applied to a signed tx if the order does not
/// supply its own GTD expiry: 5 minutes from wall-clock at submission time.
const DEFAULT_TX_EXPIRY_MS: i64 = 5 * 60 * 1_000;

/// Delay before probing an acked cancel/modify to distinguish venue no-ops
/// from account stream lag.
const ACKED_ORDER_LOOKUP_DELAY: Duration = Duration::from_secs(2);

/// Refresh the auth token this far before its issuance deadline. The
/// [`crate::signing::auth_token::DEFAULT_AUTH_TOKEN_TTL_SECS`] is 7 hours;
/// rotating at 6 hours leaves an hour of headroom for transient refresh
/// failures.
const AUTH_TOKEN_REFRESH_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(6 * 60 * 60);

// Retry budget after a scheduled rotation failure: 7 h TTL minus the 6 h
// refresh cadence leaves one hour before the old token expires.
const AUTH_TOKEN_REFRESH_RETRY_WINDOW: Duration = Duration::from_secs(60 * 60);
const AUTH_TOKEN_REFRESH_RETRY_INITIAL_DELAY: Duration = Duration::from_secs(30);

// Also used as the cadence after a retry window is exhausted
const AUTH_TOKEN_REFRESH_RETRY_MAX_DELAY: Duration = Duration::from_secs(5 * 60);
const AUTH_TOKEN_REFRESH_BACKOFF: AuthTokenRefreshBackoff = AuthTokenRefreshBackoff {
    initial_delay: AUTH_TOKEN_REFRESH_RETRY_INITIAL_DELAY,
    max_delay: AUTH_TOKEN_REFRESH_RETRY_MAX_DELAY,
    window: AUTH_TOKEN_REFRESH_RETRY_WINDOW,
};
const WS_CONSUMER_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(2);
// Bounds the informational tier-detection call so a slow or failing
// `/account` endpoint cannot stall connect for the HTTP retry budget.
const ACCOUNT_TIER_DETECT_TIMEOUT: Duration = Duration::from_secs(10);

/// Attribution window for a bare venue error frame. The frame carries no
/// `tx_hash` or cloid; if the oldest pending sendTx was submitted within
/// this window we attribute and emit `OrderRejected`. Outside the window
/// the existing submit-timeout drives expiry.
const SENDTX_BARE_ERROR_WINDOW_MS: u64 = 1_000;
const INTEGRATOR_AUTO_APPROVAL_MAX_TTL_MS: i64 = 5 * 365 * 24 * 60 * 60 * 1_000;
const INTEGRATOR_AUTO_APPROVAL_MAX_FEE_TICK: u32 = 0;

#[derive(Debug)]
pub struct LighterExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: LighterExecClientConfig,
    emitter: ExecutionEventEmitter,
    credential: Option<Credential>,
    http_client: LighterHttpClient,
    ws_client: LighterWebSocketClient,
    tx_rate_limiter: Arc<LighterTxRateLimiter>,
    tx_send_sequencer: TxSendSequencer,
    registry: Arc<MarketRegistry>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    cancellation_token: CancellationToken,
    /// WebSocket dispatch state: cloid translation tables, nonce manager,
    /// and the cached AccountState that backs `query_account`. Lives in
    /// [`crate::websocket::dispatch`].
    dispatch: WsDispatchState,
    /// Latches a burst of exhausted allocations into one venue nonce fetch.
    nonce_recovery_inflight: Arc<AtomicBool>,
}

impl LighterExecutionClient {
    /// Creates a new [`LighterExecutionClient`] instance.
    ///
    /// Resolves credentials from `config` or the matching environment
    /// variables (see [`crate::common::credential`]). Missing credentials
    /// degrade to an unauthenticated client that can bootstrap instruments
    /// but cannot submit transactions; the constructor returns an error if
    /// supplied values are malformed.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize or if any
    /// supplied credential value cannot be parsed.
    pub fn new(core: ExecutionClientCore, config: LighterExecClientConfig) -> anyhow::Result<Self> {
        let credential = Credential::resolve(
            config.private_key.clone(),
            config.account_index,
            config.api_key_index,
            config.environment,
        )
        .context("failed to resolve Lighter credentials")?;

        let registry = Arc::new(MarketRegistry::new());

        // One transaction limiter shared across the HTTP and WebSocket sendTx
        // paths so their combined rate honours the single per-account venue bucket.
        let tx_rate_limiter = build_tx_rate_limiter(config.sendtx_quota_per_min);

        let raw_http = LighterRawHttpClient::new_with_quotas(
            config.environment,
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.proxy_url.clone(),
            resolve_quota(config.rest_quota_per_min),
            Some(Arc::clone(&tx_rate_limiter)),
        )
        .context("failed to construct Lighter raw HTTP client")?;
        let http_client =
            LighterHttpClient::from_raw_with_registry(raw_http, Arc::clone(&registry));

        let ws_client = LighterWebSocketClient::new(
            config.base_url_ws.clone(),
            config.environment,
            Arc::clone(&registry),
            config.transport_backend,
            config.proxy_url.clone(),
        );

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Margin,
            None,
        );
        let dispatch = WsDispatchState::new();
        for market_index in &config.active_markets {
            dispatch.note_active_market(*market_index);
        }

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            credential,
            http_client,
            ws_client,
            tx_rate_limiter,
            tx_send_sequencer: TxSendSequencer::new(),
            registry,
            pending_tasks: Mutex::new(Vec::new()),
            ws_stream_handle: Mutex::new(None),
            cancellation_token: CancellationToken::new(),
            dispatch,
            nonce_recovery_inflight: Arc::new(AtomicBool::new(false)),
        })
    }

    /// Returns a reference to the configuration.
    #[must_use]
    pub fn config(&self) -> &LighterExecClientConfig {
        &self.config
    }

    /// Returns `true` when the client holds resolved Lighter credentials.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Returns `true` when every background task spawned by this client has
    /// completed. Useful in tests to wait for fire-and-forget HTTP work.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned, which can only occur if a
    /// task holding the lock previously panicked.
    #[must_use]
    pub fn pending_tasks_all_finished(&self) -> bool {
        let tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.iter().all(|h| h.is_finished())
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|h| !h.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    async fn ensure_instruments_initialized_async(&self) -> anyhow::Result<()> {
        if self.core.instruments_initialized() {
            return Ok(());
        }

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to request Lighter instruments")?;

        let ws_cache: Vec<(i16, InstrumentAny)> = instruments
            .iter()
            .filter_map(|instrument| {
                self.registry
                    .market_index(&instrument.id())
                    .map(|market_index| (market_index, instrument.clone()))
            })
            .collect();
        self.ws_client.cache_instruments(ws_cache);
        cache_instruments_for_reports(&instruments);

        log::debug!(
            "Bootstrapped {} Lighter instruments ({} registry entries)",
            instruments.len(),
            self.registry.len(),
        );

        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn await_account_streams_ready(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let timeout = Duration::from_secs_f64(timeout_secs);
        self.dispatch.account_streams_ready.await_all(timeout).await
    }

    async fn refresh_nonce(&self) -> anyhow::Result<()> {
        let Some(credential) = &self.credential else {
            return Ok(());
        };

        let response = self
            .http_client
            .get_next_nonce(credential.account_index(), credential.api_key_index())
            .await
            .context("failed to fetch Lighter nextNonce")?;

        self.dispatch.nonce_manager.refresh(
            credential.account_index(),
            credential.api_key_index(),
            response.nonce,
        );

        // Release the latch in case a disconnect aborted recovery mid-task
        self.nonce_recovery_inflight.store(false, Ordering::Release);

        log::debug!(
            "Refreshed Lighter nonce baseline: account_index={}, api_key_index={}, next_nonce={}",
            credential.account_index(),
            credential.api_key_index(),
            response.nonce,
        );
        Ok(())
    }

    // Logs the venue-reported account tier in blue. Informational only: the
    // active quotas are resolved from config at construction, never raised here
    // (the higher venue limits require registering the caller IP, so the tier
    // alone does not guarantee them). The call is bounded by
    // ACCOUNT_TIER_DETECT_TIMEOUT and failures are swallowed, so detection
    // cannot fail connect or stall it for the HTTP retry budget.
    async fn detect_account_tier(&self) {
        let Some(credential) = &self.credential else {
            return;
        };
        let account_index = credential.account_index();

        let detail = match tokio::time::timeout(
            ACCOUNT_TIER_DETECT_TIMEOUT,
            self.http_client.get_account_detail(account_index),
        )
        .await
        {
            Ok(Ok(detail)) => detail,
            Ok(Err(e)) => {
                log::warn!(
                    "Failed to detect Lighter account tier for account_index={account_index}; \
                     continuing at the configured REST quota: {e}"
                );
                return;
            }
            Err(_) => {
                log::warn!(
                    "Lighter account tier detection timed out after {}s for \
                     account_index={account_index}; continuing at the configured REST quota",
                    ACCOUNT_TIER_DETECT_TIMEOUT.as_secs()
                );
                return;
            }
        };

        let code = detail.account_type;
        let tier = LighterAccountTier::from_code(code);
        let standard_rest = LighterAccountTier::Standard
            .documented_rest_quota_per_min()
            .unwrap_or(60);
        let (active_rest, cross_check) =
            tier_quota_report(tier, self.config.rest_quota_per_min, standard_rest);

        log_info!(
            "Lighter execution account {account_index} reported tier {tier} \
             (account_type={code}); active REST quota {active_rest} req/min",
            color = LogColor::Blue
        );

        match cross_check {
            Some(TierCrossCheck::AboveTier { documented }) => {
                log::warn!(
                    "Configured Lighter rest_quota_per_min={active_rest} exceeds the {tier} tier \
                     limit of {documented} req/min; the venue may reject requests unless the \
                     caller IP is registered for the higher limit"
                );
            }
            Some(TierCrossCheck::RaiseHint { documented }) => {
                log_info!(
                    "Lighter {tier} tier permits up to {documented} REST req/min; set \
                     rest_quota_per_min (and register the caller IP with Lighter) to use it",
                    color = LogColor::Blue
                );
            }
            None => {}
        }
    }

    /// Returns `Ok(true)` if this credential's `api_key_index` is maker-only.
    /// Maker-only keys cannot submit `ApproveIntegrator`, so the caller skips
    /// the integrator auto-approval when `true`.
    async fn is_maker_only_api_key(&self, credential: &Credential) -> anyhow::Result<bool> {
        let auth_token = build_auth_token_for(credential)
            .context("failed to mint Lighter auth token for maker-only check")?;
        let response = self
            .http_client
            .get_maker_only_api_keys(credential.account_index(), auth_token)
            .await
            .context("failed to query getMakerOnlyApiKeys")?;
        let api_key_index = i64::from(credential.api_key_index());
        Ok(response.api_key_indexes.contains(&api_key_index))
    }

    async fn submit_integrator_auto_approval(&self) -> anyhow::Result<()> {
        let Some(credential) = &self.credential else {
            return Ok(());
        };

        let mut maker_only_check_failed = false;

        match self.is_maker_only_api_key(credential).await {
            Ok(true) => {
                log::warn!(
                    "Skipping Lighter integrator auto-approval: api_key_index={} is maker-only; \
                     ensure the account has been approved by a non-maker-only key",
                    credential.api_key_index(),
                );
                return Ok(());
            }
            Ok(false) => {}
            Err(e) => {
                maker_only_check_failed = true;
                log::debug!(
                    "Lighter maker-only api key check failed; attempting integrator approval \
                     anyway: {e:?}"
                );
            }
        }

        let mut approval = self.prepare_integrator_auto_approval(credential)?;

        let request = LighterSendTxRequest::new(
            LighterTxType::ApproveIntegrator as u8,
            approval.tx_info.clone(),
        );

        approval.send_reservation.wait_for_turn().await;

        let response = self.http_client.send_tx(&request).await.with_context(|| {
            let hint = if maker_only_check_failed {
                " (maker-only pre-flight check failed earlier; venue may reject with 62007 \
                 if this key is maker-only)"
            } else {
                ""
            };
            format!(
                "failed to submit Lighter integrator approval nonce={} api_key_index={}{hint}",
                approval.nonce, approval.api_key_index,
            )
        })?;

        approval.send_reservation.release();

        log::debug!(
            "Submitted Lighter integrator approval: integrator={}, nonce={}, \
             api_key_index={}, approval_expiry={}, tx_hash={}",
            LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
            approval.nonce,
            approval.api_key_index,
            approval.approval_expiry,
            response.tx_hash,
        );
        Ok(())
    }

    fn prepare_integrator_auto_approval(
        &self,
        credential: &Credential,
    ) -> anyhow::Result<PreparedIntegratorApproval> {
        let ReservedTxContext {
            context,
            send_reservation,
        } = self.build_tx_context(credential)?;

        let now_ms = (self.clock.get_time_ns().as_u64() as i64) / 1_000_000;
        let approval_expiry = now_ms.saturating_add(INTEGRATOR_AUTO_APPROVAL_MAX_TTL_MS);
        let nonce = context.nonce;
        let api_key_index = context.api_key_index;

        let tx = ApproveIntegratorTxInfo {
            context,
            integrator_account_index: LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX as i64,
            max_perps_taker_fee: INTEGRATOR_AUTO_APPROVAL_MAX_FEE_TICK,
            max_perps_maker_fee: INTEGRATOR_AUTO_APPROVAL_MAX_FEE_TICK,
            max_spot_taker_fee: INTEGRATOR_AUTO_APPROVAL_MAX_FEE_TICK,
            max_spot_maker_fee: INTEGRATOR_AUTO_APPROVAL_MAX_FEE_TICK,
            approval_expiry,
            skip_nonce: 0,
        };

        let signed = sign_tx(
            &tx,
            lighter_chain_id(self.config.environment),
            &credential.private_key()?,
            fresh_k(),
        );
        let tx_info = TxInfoJson::approve_integrator(&tx, &signed, "");

        Ok(PreparedIntegratorApproval {
            tx_info,
            nonce,
            api_key_index,
            approval_expiry,
            send_reservation,
        })
    }

    async fn spawn_ws_consumer(&mut self) -> anyhow::Result<()> {
        // Local clone owns the handler `task_handle` until post-connect
        // setup succeeds. Transferring earlier would leave failures unable
        // to drain the task through the clone's `disconnect()`.
        let mut ws_client = self.ws_client.clone();
        ws_client
            .connect()
            .await
            .context("failed to connect to Lighter WebSocket")?;

        // Wrapped so any failure routes through the clone's `disconnect()`
        // (which still owns the handler task); mirrors Hyperliquid's
        // `post_ws` block.
        let post_connect = async {
            ws_client
                .wait_until_active(10.0)
                .await
                .context("Lighter WebSocket did not reach active state")?;

            if let Some(credential) = &self.credential {
                let auth_token = build_auth_token_for(credential)
                    .context("failed to mint Lighter auth token")?;
                let account_index = credential.account_index();

                ws_client
                    .set_execution_context(self.core.account_id, account_index)
                    .await
                    .map_err(|e| anyhow::anyhow!("failed to set Lighter execution context: {e}"))?;

                // Subscribe to the five account-scoped streams the consumption
                // loop converts into typed reports. The handler merges
                // `account_all_assets` and `user_stats` into a single
                // AccountState (see websocket/account_state.rs).
                let channels = [
                    LighterWsChannel::AccountAllOrders(account_index),
                    LighterWsChannel::AccountAllTrades(account_index),
                    LighterWsChannel::AccountAllPositions(account_index),
                    LighterWsChannel::AccountAllAssets(account_index),
                    LighterWsChannel::UserStats(account_index),
                ];

                for channel in channels {
                    ws_client
                        .subscribe_account(channel.clone(), auth_token.clone())
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!(
                                "failed to subscribe to Lighter account channel {channel:?}: {e}",
                            )
                        })?;
                }

                log::debug!("Subscribed to Lighter account streams: account_index={account_index}",);
            } else {
                log::warn!(
                    "Lighter execution client has no credentials: account streams not subscribed; \
                     typed execution reports will not flow"
                );
            }

            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_connect.await {
            log::warn!("Lighter post-connect setup failed, tearing down WS: {e}");
            if let Err(disconnect_err) = ws_client.disconnect().await {
                log::error!(
                    "Error disconnecting Lighter WebSocket during connect teardown: {disconnect_err}"
                );
            }
            return Err(e);
        }

        if let Some(handle) = ws_client.take_task_handle() {
            self.ws_client.set_task_handle(handle);
        }

        let cancellation_token = self.cancellation_token.clone();
        let emitter = self.emitter.clone();
        let dispatch = self.dispatch.clone();
        let credential_for_loop = self.credential.clone();
        let http_client_for_loop = self.http_client.clone();
        let registry_for_loop = Arc::clone(&self.registry);
        let account_id_for_loop = self.core.account_id;
        let clock_for_loop = self.clock;

        let task = get_runtime().spawn(async move {
            log::debug!("Lighter execution WebSocket consumption loop started");

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Lighter execution consumption loop cancelled");
                        break;
                    }
                    msg_opt = ws_client.next_event() => {
                        match msg_opt {
                            Some(NautilusWsMessage::ExecutionReports(reports)) => {
                                let mut order_count = 0_usize;
                                let mut fill_count = 0_usize;
                                let trader_id = emitter.trader_id();
                                let account_index = credential_for_loop
                                    .as_ref()
                                    .map(|c| c.account_index());

                                for report in reports {
                                    match report {
                                        ExecutionReport::Order(order) => {
                                            order_count += 1;
                                            dispatch_lighter_order(
                                                &order,
                                                &dispatch,
                                                &emitter,
                                                &registry_for_loop,
                                                account_id_for_loop,
                                                trader_id,
                                                clock_for_loop.get_time_ns(),
                                            );
                                        }
                                        ExecutionReport::Fill(trade) => {
                                            fill_count += 1;
                                            dispatch_lighter_trade(
                                                &trade,
                                                &dispatch,
                                                &emitter,
                                                &registry_for_loop,
                                                account_id_for_loop,
                                                trader_id,
                                                account_index,
                                                clock_for_loop.get_time_ns(),
                                            );
                                        }
                                    }
                                }
                                log::debug!(
                                    "Lighter execution batch: orders={order_count} fills={fill_count}",
                                );
                            }
                            Some(NautilusWsMessage::PositionSnapshot {
                                reports,
                                skipped_market_ids,
                            }) => {
                                // Replace even when empty, but keep rows the
                                // handler could not parse or map.
                                for r in &reports {
                                    if let Some(idx) =
                                        registry_for_loop.market_index(&r.instrument_id)
                                    {
                                        dispatch.note_active_market(idx);
                                    }
                                }
                                let position_count = reports.len();
                                let retained_positions: Vec<InstrumentId> = skipped_market_ids
                                    .iter()
                                    .filter_map(|market_id| {
                                        registry_for_loop.instrument_id(*market_id)
                                    })
                                    .collect();
                                let removed = if retained_positions.is_empty() {
                                    dispatch.replace_positions(&reports)
                                } else {
                                    dispatch.replace_positions_except(&reports, &retained_positions)
                                };
                                log::debug!(
                                    "Lighter position snapshot: positions={position_count}, skipped_markets={}, removed={}",
                                    skipped_market_ids.len(),
                                    removed.len(),
                                );

                                for r in reports {
                                    log::debug!(
                                        "Lighter PositionStatusReport: instrument={} side={:?} qty={}",
                                        r.instrument_id,
                                        r.position_side,
                                        r.quantity,
                                    );
                                    emitter.send_position_report(r);
                                }

                                // Emit a flat report for any instrument the
                                // venue dropped from this snapshot so the
                                // engine sees the close. Without this, an
                                // externally-closed position lingers in the
                                // engine cache even though the dispatch
                                // cache cleared it.
                                let now = clock_for_loop.get_time_ns();

                                for instrument_id in removed {
                                    let flat = PositionStatusReport::new(
                                        account_id_for_loop,
                                        instrument_id,
                                        PositionSideSpecified::Flat,
                                        Quantity::zero(0),
                                        now,
                                        now,
                                        Some(UUID4::new()),
                                        None,
                                        None,
                                    );
                                    emitter.send_position_report(flat);
                                }
                            }
                            Some(NautilusWsMessage::AccountState(state)) => {
                                log::debug!(
                                    "Lighter AccountState: balances={} margins={}",
                                    state.balances.len(),
                                    state.margins.len(),
                                );
                                // Cache so query_account can serve a recent
                                // snapshot without a REST round-trip; Lighter
                                // does not currently expose a REST account
                                // endpoint that would make a fresh fetch
                                // possible.
                                dispatch.cache_account_state((*state).clone());
                                emitter.send_account_state(*state);
                            }
                            Some(NautilusWsMessage::Reconnected) => {
                                // Subscriptions are restored by
                                // `LighterWebSocketClient`'s reconnect logic;
                                // the execution context is preserved by the
                                // handler across reconnects. Refresh the nonce
                                // baseline since the venue's expected next
                                // nonce may have advanced while we were
                                // disconnected.
                                log::debug!("Lighter WebSocket reconnected (execution stream)");

                                // No cache touch here: the next venue
                                // `account_all_positions` snapshot is
                                // authoritative and drives the diff. A
                                // synthetic flat from the lifecycle would
                                // produce a false close+reopen on a healthy
                                // flap. Trade-off: between reconnect and the
                                // next snapshot (~<1s typically),
                                // `generate_position_status_reports` may
                                // return stale data.

                                // Drained creates have unknown outcomes;
                                // reconciliation resolves them, so warn
                                // rather than emit rejections.
                                let stale = dispatch.drain_pending_sendtx();
                                if !stale.is_empty() {
                                    log::warn!(
                                        "Discarded {} pending sendTx entries on reconnect; \
                                         order state recovers via reconciliation",
                                        stale.len(),
                                    );

                                    for pending in &stale {
                                        if let PendingSendTxKind::Create { order, .. } =
                                            &pending.kind
                                        {
                                            log::warn!(
                                                "Lighter sendTx outcome unknown after reconnect \
                                                 for {}",
                                                order.client_order_id(),
                                            );
                                        }
                                    }
                                }

                                if let Some(credential) = &credential_for_loop {
                                    match http_client_for_loop
                                        .get_next_nonce(
                                            credential.account_index(),
                                            credential.api_key_index(),
                                        )
                                        .await
                                    {
                                        Ok(response) => {
                                            dispatch.nonce_manager.refresh(
                                                credential.account_index(),
                                                credential.api_key_index(),
                                                response.nonce,
                                            );
                                            log::debug!(
                                                "Refreshed Lighter nonce after reconnect: \
                                                 account_index={}, next_nonce={}",
                                                credential.account_index(),
                                                response.nonce,
                                            );
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to refresh Lighter nonce after reconnect: {e}",
                                            );
                                        }
                                    }
                                }
                            }
                            Some(NautilusWsMessage::SendTxAck { tx_hash, code }) => {
                                let account_index = credential_for_loop
                                    .as_ref()
                                    .map(|c| c.account_index());
                                let acked = handle_send_tx_ack(
                                    &dispatch,
                                    account_index,
                                    code,
                                    tx_hash.as_deref(),
                                );

                                if let (Some(pending), Some(credential)) =
                                    (acked, credential_for_loop.clone())
                                {
                                    spawn_acked_order_probe(
                                        &pending,
                                        AckedOrderProbeContext {
                                            http_client: http_client_for_loop.clone(),
                                            registry: Arc::clone(&registry_for_loop),
                                            credential,
                                            dispatch: dispatch.clone(),
                                            emitter: emitter.clone(),
                                            account_id: account_id_for_loop,
                                            clock: clock_for_loop,
                                            cancellation_token: cancellation_token.clone(),
                                        },
                                    );
                                }
                            }
                            Some(NautilusWsMessage::SendTxRejected {
                                source,
                                code,
                                message,
                                tx_hash,
                            }) => {
                                let account_index = credential_for_loop
                                    .as_ref()
                                    .map(|c| c.account_index());
                                let needs_nonce_resync = handle_send_tx_rejection(
                                    &dispatch,
                                    &emitter,
                                    account_index,
                                    clock_for_loop.get_time_ns(),
                                    source,
                                    code,
                                    &message,
                                    tx_hash.as_deref(),
                                );

                                // Invalid nonce means the sequential stream is
                                // wedged on a burned nonce; only a hard refresh
                                // moves allocation back down.
                                if needs_nonce_resync
                                    && let Some(credential) = &credential_for_loop
                                {
                                    match http_client_for_loop
                                        .get_next_nonce(
                                            credential.account_index(),
                                            credential.api_key_index(),
                                        )
                                        .await
                                    {
                                        Ok(response) => {
                                            dispatch.nonce_manager.refresh(
                                                credential.account_index(),
                                                credential.api_key_index(),
                                                response.nonce,
                                            );
                                            log::info!(
                                                "Hard-refreshed Lighter nonce after invalid-nonce \
                                                 rejection: account_index={}, next_nonce={}",
                                                credential.account_index(),
                                                response.nonce,
                                            );
                                        }
                                        Err(e) => {
                                            log::error!(
                                                "Failed to refresh Lighter nonce after \
                                                 invalid-nonce rejection: {e}",
                                            );
                                        }
                                    }
                                }
                            }
                            Some(NautilusWsMessage::Raw(value)) => {
                                log::debug!("Unhandled Lighter raw frame on execution stream: {value}");
                            }
                            Some(NautilusWsMessage::AccountStreamFirstFrame(stream)) => {
                                // FIFO with preceding reports on the same
                                // channel: any typed reports the handler
                                // emitted for this frame have already been
                                // applied by the cases above, so marking
                                // here is safe to unblock `await_all`.
                                match stream {
                                    AccountStream::Orders => {
                                        dispatch.account_streams_ready.mark_orders();
                                    }
                                    AccountStream::Trades => {
                                        dispatch.account_streams_ready.mark_trades();
                                    }
                                    AccountStream::Positions => {
                                        dispatch.account_streams_ready.mark_positions();
                                    }
                                    AccountStream::Assets => {
                                        dispatch.account_streams_ready.mark_assets();
                                    }
                                    AccountStream::UserStats => {
                                        dispatch.account_streams_ready.mark_user_stats();
                                    }
                                }
                            }
                            // Public market data variants reach the execution
                            // stream only if the user shares a websocket clone
                            // with the data client (no production caller does).
                            Some(
                                NautilusWsMessage::Trades(_)
                                | NautilusWsMessage::Quote(_)
                                | NautilusWsMessage::Deltas(_)
                                | NautilusWsMessage::Depth10(_)
                                | NautilusWsMessage::Bar(_)
                                | NautilusWsMessage::MarkPrice(_)
                                | NautilusWsMessage::IndexPrice(_)
                                | NautilusWsMessage::FundingRate(_),
                            ) => {}
                            None => {
                                log::debug!("Lighter execution next_event returned None");
                                tokio::select! {
                                    () = cancellation_token.cancelled() => {
                                        log::debug!(
                                            "Lighter execution consumption loop cancelled"
                                        );
                                        break;
                                    }
                                    () = tokio::time::sleep(Duration::from_secs(1)) => {}
                                }
                            }
                        }
                    }
                }
            }

            log::debug!("Lighter execution WebSocket consumption loop finished");
        });

        let mut handle = self.ws_stream_handle.lock().expect(MUTEX_POISONED);
        *handle = Some(task);
        drop(handle);

        if let Some(credential) = &self.credential {
            self.spawn_auth_token_refresh(credential.clone());
        }

        Ok(())
    }

    fn spawn_auth_token_refresh(&self, credential: Credential) {
        let ws_client = self.ws_client.clone();
        let cancellation_token = self.cancellation_token.clone();
        let account_index = credential.account_index();
        let channels = auth_token_rotation_channels(account_index);

        get_runtime().spawn(async move {
            log::debug!(
                "Lighter auth-token refresh task started: interval={}s, account_index={account_index}",
                AUTH_TOKEN_REFRESH_INTERVAL.as_secs(),
            );

            let mut next_refresh_delay = AUTH_TOKEN_REFRESH_INTERVAL;

            loop {
                if !sleep_or_auth_token_refresh_cancelled(
                    next_refresh_delay,
                    &cancellation_token,
                )
                .await
                {
                    log::debug!("Lighter auth-token refresh task cancelled");
                    break;
                }

                let outcome = refresh_auth_token_until_rotated(
                    &credential,
                    &channels,
                    &cancellation_token,
                    AUTH_TOKEN_REFRESH_BACKOFF,
                    |credential| -> anyhow::Result<String> { build_auth_token_for(credential) },
                    |channel, token| {
                        let ws_client = ws_client.clone();
                        async move { ws_client.subscribe_account(channel, token).await }
                    },
                )
                .await;

                if let Some(delay) = auth_token_refresh_next_delay(outcome) {
                    if outcome == AuthTokenRefreshOutcome::Exhausted {
                        log::error!(
                            "Lighter auth-token rotation exhausted retry window; retrying again in {}s: account_index={account_index}",
                            delay.as_secs(),
                        );
                    }
                    next_refresh_delay = delay;
                } else {
                    log::debug!("Lighter auth-token refresh task cancelled");
                    break;
                }
            }
        });
    }

    // Per-order `params["market_order_slippage_bps"]` overrides the config default.
    fn resolve_slippage_bps(&self, params: Option<&Params>) -> u32 {
        params
            .and_then(|p| p.get_u64("market_order_slippage_bps"))
            .map_or(self.config.market_order_slippage_bps, |v| v as u32)
    }

    fn build_tx_context(&self, credential: &Credential) -> anyhow::Result<ReservedTxContext> {
        let nonce = match self
            .dispatch
            .nonce_manager
            .next_nonce(credential.account_index(), credential.api_key_index())
        {
            Ok(nonce) => nonce,
            Err(e @ NonceError::SkipWindowExhausted { .. }) => {
                // Lost acks leave the baseline stale; resync from the venue so
                // later commands recover. The fetch is async; this command fails.
                self.spawn_nonce_window_recovery(credential);
                anyhow::bail!("failed to allocate Lighter nonce: {e}");
            }
            Err(e) => anyhow::bail!("failed to allocate Lighter nonce: {e}"),
        };

        let now_ns = self.clock.get_time_ns().as_u64() as i64;
        let expired_at = (now_ns / 1_000_000).saturating_add(DEFAULT_TX_EXPIRY_MS);
        let send_reservation = self.tx_send_sequencer.reserve(
            credential.account_index(),
            credential.api_key_index(),
            nonce,
        );

        let context = TxContext {
            account_index: credential.account_index(),
            api_key_index: credential.api_key_index(),
            nonce,
            expired_at,
        };

        Ok(ReservedTxContext {
            context,
            send_reservation,
        })
    }

    fn spawn_nonce_window_recovery(&self, credential: &Credential) {
        if self.nonce_recovery_inflight.swap(true, Ordering::AcqRel) {
            return;
        }

        let inflight = Arc::clone(&self.nonce_recovery_inflight);
        let http_client = self.http_client.clone();
        let dispatch = self.dispatch.clone();
        let account_index = credential.account_index();
        let api_key_index = credential.api_key_index();

        self.spawn_task("nonce_window_recovery", async move {
            let result = http_client
                .get_next_nonce(account_index, api_key_index)
                .await;
            inflight.store(false, Ordering::Release);

            match result {
                Ok(response) => {
                    // Monotonic sync, not `refresh`: a hard reset could
                    // reissue nonces already signed into in-flight txs.
                    let _ = dispatch.nonce_manager.sync_from_venue(
                        account_index,
                        api_key_index,
                        response.nonce,
                    );
                    log::info!(
                        "Resynced Lighter nonce baseline after skip-window exhaustion: \
                         account_index={account_index}, api_key_index={api_key_index}, \
                         next_nonce={}",
                        response.nonce,
                    );
                }
                Err(e) => {
                    log::error!(
                        "Failed to resync Lighter nonce after skip-window exhaustion: {e}",
                    );
                }
            }
            Ok(())
        });
    }

    fn dispatch_signed_create_order(
        &self,
        order: &OrderAny,
        credential: &Credential,
        slippage_bps: u32,
    ) -> anyhow::Result<()> {
        let prepared = self.prepare_signed_create_order(order, credential, slippage_bps)?;
        let PreparedCreateOrder {
            order,
            client_order_index,
            tx_info,
            nonce,
            api_key_index,
            tx_hash,
            mut send_reservation,
        } = prepared;
        let ws_client = self.ws_client.clone();
        let dispatch = self.dispatch.clone();
        let tx_rate_limiter = self.tx_rate_limiter.clone();
        let credential = credential.clone();
        let client_order_id = order.client_order_id();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.emitter.emit_order_submitted(&order);

        self.spawn_task("submit_order", async move {
            log::debug!("Lighter submit_order: queueing CreateOrder tx for {client_order_id}");
            send_reservation.wait_for_turn().await;
            // Pace before enqueueing so the pending FIFO order matches the send
            // order and `submitted_at` is fresh for ack/rejection attribution.
            await_tx_quota(&tx_rate_limiter).await;
            dispatch.enqueue_pending_sendtx(PendingSendTx {
                kind: PendingSendTxKind::Create {
                    order: Box::new(order.clone()),
                    client_order_index,
                },
                submitted_at: clock.get_time_ns(),
                nonce,
                api_key_index,
                tx_hash,
            });

            if let Err(e) = ws_client
                .send_tx(LighterTxType::CreateOrder as u8, tx_info)
                .await
            {
                let reason = format!("Lighter submit_order dispatch failed: {e}");
                log::error!("{reason} for {client_order_id}");
                dispatch.remove_pending_sendtx_by_nonce(nonce);
                rollback_tx_dispatch_create(
                    &dispatch,
                    &credential,
                    Some(client_order_index),
                    &client_order_id,
                    nonce,
                );

                emitter.emit_order_rejected(&order, &reason, clock.get_time_ns(), false);
            }
            send_reservation.release();
            Ok(())
        });

        Ok(())
    }

    fn prepare_signed_create_order(
        &self,
        order: &OrderAny,
        credential: &Credential,
        slippage_bps: u32,
    ) -> anyhow::Result<PreparedCreateOrder> {
        let instrument_id = order.instrument_id();
        let market_index = self.registry.market_index(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("no Lighter market_index registered for instrument {instrument_id}")
        })?;

        let instrument = self.core.cache().try_instrument(&instrument_id)?.clone();

        let order_kind = nautilus_to_lighter_order_type(order.order_type())?;

        let tif = nautilus_to_lighter_tif(
            order.order_type(),
            order.time_in_force(),
            order.is_post_only(),
        )?;
        let now_ms = (self.clock.get_time_ns().as_u64() / 1_000_000) as i64;
        let order_expiry = order_expiry_for(
            order.order_type(),
            &order.time_in_force(),
            order.expire_time(),
            now_ms,
        );

        let base_amount = quantity_to_ticks(&order.quantity(), instrument.size_precision())?;

        // `quantity_to_ticks` floors sub-precision quantities to 0.
        anyhow::ensure!(
            base_amount > 0,
            "quantity `{}` rounds to 0 ticks at size_precision {}",
            order.quantity(),
            instrument.size_precision(),
        );
        let price_precision = instrument.price_precision();
        let is_buy = matches!(order.order_side(), OrderSide::Buy);

        // Lighter requires `price` on market-style orders as the worst
        // acceptable cap; derive it from far-side quote or trigger.
        let price_ticks = match order.order_type() {
            OrderType::Market => {
                let quote = self
                    .core
                    .cache()
                    .quote(&instrument_id)
                    .copied()
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "no cached quote for {instrument_id}: subscribe to quotes before submitting MARKET orders",
                        )
                    })?;
                let base = if is_buy {
                    quote.ask_price.as_decimal()
                } else {
                    quote.bid_price.as_decimal()
                };
                derive_market_order_price_ticks(base, is_buy, price_precision, slippage_bps)?
            }
            OrderType::StopMarket | OrderType::MarketIfTouched => {
                let trigger = order.trigger_price().ok_or_else(|| {
                    anyhow::anyhow!("{:?} orders require a trigger_price", order.order_type(),)
                })?;
                derive_market_order_price_ticks(
                    trigger.as_decimal(),
                    is_buy,
                    price_precision,
                    slippage_bps,
                )?
            }
            _ => order
                .price()
                .map(|p| price_to_ticks(&p, price_precision))
                .transpose()?
                .unwrap_or(0),
        };

        let trigger_price_ticks = order
            .trigger_price()
            .map(|p| price_to_ticks(&p, price_precision))
            .transpose()?
            .unwrap_or(0);

        // Conditional types: `price_to_ticks` floors sub-tick triggers to 0,
        // which Lighter would then reject.
        if matches!(
            order.order_type(),
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        ) {
            anyhow::ensure!(
                trigger_price_ticks > 0,
                "trigger_price `{:?}` rounds to 0 ticks at precision {price_precision}",
                order.trigger_price(),
            );
        }
        validate_order_amount(&instrument, order.quantity(), price_ticks, price_precision)?;

        let cloid = order.client_order_id();
        let initial_index = self.dispatch.derive_client_order_index(&cloid);
        let client_order_index = self.dispatch.register_cloid(initial_index, cloid);
        self.dispatch.register_order_identity(
            cloid,
            crate::websocket::dispatch::OrderIdentity {
                instrument_id,
                strategy_id: order.strategy_id(),
                order_side: order.order_side(),
                order_type: order.order_type(),
            },
        );

        let ReservedTxContext {
            context,
            send_reservation,
        } = match self.build_tx_context(credential) {
            Ok(context) => context,
            Err(e) => {
                self.dispatch.forget_cloid(client_order_index);
                self.dispatch.forget_order_identity(&cloid);
                return Err(e);
            }
        };

        let nonce = context.nonce;
        let api_key_index = context.api_key_index;

        let mut rollback_guard = TxDispatchGuard::new(
            self.dispatch.clone(),
            credential,
            Some(client_order_index),
            nonce,
        )
        .with_order_identity(cloid);
        let tx = CreateOrderTxInfo {
            context,
            order: OrderInfo {
                market_index,
                client_order_index,
                base_amount,
                price: price_ticks,
                is_ask: matches!(order.order_side(), OrderSide::Sell),
                order_type: order_kind as u8,
                time_in_force: tif as u8,
                reduce_only: order.is_reduce_only(),
                trigger_price: trigger_price_ticks,
                order_expiry,
            },
            attributes: integrator_attributes(),
        };

        let signed = sign_tx(
            &tx,
            lighter_chain_id(self.config.environment),
            &credential.private_key()?,
            fresh_k(),
        );

        let tx_info_str = TxInfoJson::create_order(&tx, &signed);
        let tx_info = serde_json::value::RawValue::from_string(tx_info_str)
            .context("failed to wrap signed Lighter tx_info JSON")?;
        rollback_guard.disarm();

        Ok(PreparedCreateOrder {
            order: order.clone(),
            client_order_index,
            tx_info,
            nonce,
            api_key_index,
            tx_hash: signed.tx_hash_hex(),
            send_reservation,
        })
    }

    fn dispatch_signed_cancel_order(&self, cmd: &CancelOrder, credential: &Credential) {
        let prepared = match self.prepare_signed_cancel_order(cmd, credential) {
            Ok(prepared) => prepared,
            Err(e) => {
                let reason = format!("Lighter cancel_order failed: {e}");
                if self.can_emit_order_cancel_rejected(&cmd.client_order_id) {
                    log::warn!("{reason} for {}", cmd.client_order_id);
                    self.emitter.emit_order_cancel_rejected_event(
                        cmd.strategy_id,
                        cmd.instrument_id,
                        cmd.client_order_id,
                        cmd.venue_order_id,
                        &reason,
                        self.clock.get_time_ns(),
                    );
                } else {
                    log::warn!(
                        "{reason} for {}; suppressing OrderCancelRejected because order is not PendingCancel",
                        cmd.client_order_id,
                    );
                }
                return;
            }
        };
        let PreparedCancelOrder {
            client_order_id,
            strategy_id,
            instrument_id,
            venue_order_id,
            tx_info,
            nonce,
            api_key_index,
            tx_hash,
            mut send_reservation,
        } = prepared;
        let emit_cancel_rejected = self.can_emit_order_cancel_rejected(&client_order_id);

        let ws_client = self.ws_client.clone();
        let dispatch = self.dispatch.clone();
        let credential = credential.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let tx_rate_limiter = self.tx_rate_limiter.clone();

        self.spawn_task("cancel_order", async move {
            send_reservation.wait_for_turn().await;
            await_tx_quota(&tx_rate_limiter).await;
            dispatch.enqueue_pending_sendtx(PendingSendTx {
                kind: if emit_cancel_rejected {
                    PendingSendTxKind::Cancel {
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                    }
                } else {
                    PendingSendTxKind::Other
                },
                submitted_at: clock.get_time_ns(),
                nonce,
                api_key_index,
                tx_hash,
            });

            if let Err(e) = ws_client
                .send_tx(LighterTxType::CancelOrder as u8, tx_info)
                .await
            {
                let reason = format!("Lighter cancel_order dispatch failed: {e}");
                log::error!("{reason} for {client_order_id}");
                dispatch.remove_pending_sendtx_by_nonce(nonce);
                rollback_tx_dispatch(&dispatch, &credential, None, nonce);

                if emit_cancel_rejected {
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &reason,
                        clock.get_time_ns(),
                    );
                } else {
                    log::warn!(
                        "{reason} for {client_order_id}; suppressing OrderCancelRejected because order is not PendingCancel",
                    );
                }
            }

            send_reservation.release();
            Ok(())
        });
    }

    fn can_emit_order_cancel_rejected(&self, client_order_id: &ClientOrderId) -> bool {
        self.core
            .cache()
            .order(client_order_id)
            .is_none_or(|order| order.is_pending_cancel())
    }

    fn prepare_signed_cancel_order(
        &self,
        cmd: &CancelOrder,
        credential: &Credential,
    ) -> anyhow::Result<PreparedCancelOrder> {
        let market_index = self
            .registry
            .market_index(&cmd.instrument_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Lighter market_index registered for instrument {}",
                    cmd.instrument_id,
                )
            })?;

        self.core.cache().try_order(&cmd.client_order_id)?;

        // Lighter cancel_order targets a single order by venue order_id.
        // The map is populated on the first OrderStatusReport for the cloid.
        let voi = cmd
            .venue_order_id
            .or_else(|| self.dispatch.lookup_venue_order_id(&cmd.client_order_id))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot cancel Lighter order {}: venue order_id not yet known \
                     (await OrderAccepted before issuing cancel)",
                    cmd.client_order_id,
                )
            })?;

        let venue_index: i64 = voi
            .as_str()
            .parse()
            .with_context(|| format!("Lighter venue_order_id `{voi}` is not an integer index"))?;

        let ReservedTxContext {
            context,
            send_reservation,
        } = self.build_tx_context(credential)?;

        let captured_nonce = context.nonce;
        let captured_api_key_index = context.api_key_index;
        let mut rollback_guard =
            TxDispatchGuard::new(self.dispatch.clone(), credential, None, captured_nonce);
        let tx = CancelOrderTxInfo {
            context,
            market_index,
            index: venue_index,
            skip_nonce: 0,
        };

        let signed = sign_tx(
            &tx,
            lighter_chain_id(self.config.environment),
            &credential.private_key()?,
            fresh_k(),
        );
        let tx_info_str = TxInfoJson::cancel_order(&tx, &signed);
        let tx_info = serde_json::value::RawValue::from_string(tx_info_str)
            .context("failed to wrap signed Lighter cancel tx_info JSON")?;
        rollback_guard.disarm();

        Ok(PreparedCancelOrder {
            client_order_id: cmd.client_order_id,
            strategy_id: cmd.strategy_id,
            instrument_id: cmd.instrument_id,
            venue_order_id: Some(voi),
            tx_info,
            nonce: captured_nonce,
            api_key_index: captured_api_key_index,
            tx_hash: signed.tx_hash_hex(),
            send_reservation,
        })
    }

    fn dispatch_signed_modify_order(&self, cmd: &ModifyOrder, credential: &Credential) {
        let prepared = match self.prepare_signed_modify_order(cmd, credential) {
            Ok(prepared) => prepared,
            Err(e) => {
                let reason = format!("Lighter modify_order failed: {e}");
                log::warn!("{reason} for {}", cmd.client_order_id);
                self.emitter.emit_order_modify_rejected_event(
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    cmd.venue_order_id,
                    &reason,
                    self.clock.get_time_ns(),
                );
                return;
            }
        };
        let PreparedModifyOrder {
            client_order_id,
            strategy_id,
            instrument_id,
            venue_order_id,
            tx_info,
            nonce,
            api_key_index,
            tx_hash,
            mut send_reservation,
        } = prepared;

        let ws_client = self.ws_client.clone();
        let dispatch = self.dispatch.clone();
        let credential = credential.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let tx_rate_limiter = self.tx_rate_limiter.clone();

        self.spawn_task("modify_order", async move {
            send_reservation.wait_for_turn().await;
            await_tx_quota(&tx_rate_limiter).await;
            dispatch.enqueue_pending_sendtx(PendingSendTx {
                kind: PendingSendTxKind::Modify {
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                },
                submitted_at: clock.get_time_ns(),
                nonce,
                api_key_index,
                tx_hash,
            });

            if let Err(e) = ws_client
                .send_tx(LighterTxType::ModifyOrder as u8, tx_info)
                .await
            {
                let reason = format!("Lighter modify_order dispatch failed: {e}");
                log::error!("{reason} for {client_order_id}");
                dispatch.remove_pending_sendtx_by_nonce(nonce);
                rollback_tx_dispatch(&dispatch, &credential, None, nonce);
                emitter.emit_order_modify_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &reason,
                    clock.get_time_ns(),
                );
            }
            send_reservation.release();
            Ok(())
        });
    }

    fn prepare_signed_modify_order(
        &self,
        cmd: &ModifyOrder,
        credential: &Credential,
    ) -> anyhow::Result<PreparedModifyOrder> {
        let market_index = self
            .registry
            .market_index(&cmd.instrument_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "no Lighter market_index registered for instrument {}",
                    cmd.instrument_id,
                )
            })?;

        let voi = cmd
            .venue_order_id
            .or_else(|| self.dispatch.lookup_venue_order_id(&cmd.client_order_id))
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "cannot modify Lighter order {}: venue order_id not yet known \
                     (await OrderAccepted before issuing modify)",
                    cmd.client_order_id,
                )
            })?;

        let venue_index: i64 = voi
            .as_str()
            .parse()
            .with_context(|| format!("Lighter venue_order_id `{voi}` is not an integer index"))?;

        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        let instrument = self
            .core
            .cache()
            .try_instrument(&cmd.instrument_id)?
            .clone();

        let new_qty = cmd.quantity.unwrap_or(order.quantity());
        let new_price = cmd.price.or(order.price()).ok_or_else(|| {
            anyhow::anyhow!("modify_order requires a price (none on order or command)")
        })?;
        let new_trigger = cmd
            .trigger_price
            .or(order.trigger_price())
            .unwrap_or(Price::from_raw(0, instrument.price_precision()));

        let base_amount = quantity_to_ticks(&new_qty, instrument.size_precision())?;
        let price_ticks = price_to_ticks(&new_price, instrument.price_precision())?;
        let trigger_price_ticks = if new_trigger.raw == 0 {
            0
        } else {
            price_to_ticks(&new_trigger, instrument.price_precision())?
        };

        let ReservedTxContext {
            context,
            send_reservation,
        } = self.build_tx_context(credential)?;

        let captured_nonce = context.nonce;
        let captured_api_key_index = context.api_key_index;
        let mut rollback_guard =
            TxDispatchGuard::new(self.dispatch.clone(), credential, None, captured_nonce);
        let tx = ModifyOrderTxInfo {
            context,
            market_index,
            index: venue_index,
            base_amount,
            price: price_ticks,
            trigger_price: trigger_price_ticks,
            attributes: integrator_attributes(),
        };

        let signed = sign_tx(
            &tx,
            lighter_chain_id(self.config.environment),
            &credential.private_key()?,
            fresh_k(),
        );

        let tx_info_str = TxInfoJson::modify_order(&tx, &signed);
        let tx_info = serde_json::value::RawValue::from_string(tx_info_str)
            .context("failed to wrap signed Lighter modify tx_info JSON")?;
        rollback_guard.disarm();

        Ok(PreparedModifyOrder {
            client_order_id: cmd.client_order_id,
            strategy_id: cmd.strategy_id,
            instrument_id: cmd.instrument_id,
            venue_order_id: Some(voi),
            tx_info,
            nonce: captured_nonce,
            api_key_index: captured_api_key_index,
            tx_hash: signed.tx_hash_hex(),
            send_reservation,
        })
    }

    /// Submit Lighter's native `UpdateLeverage` tx (`tx_type = 20`).
    ///
    /// Changes the initial margin fraction and the position margin mode for
    /// the given market. `initial_margin_fraction` is in venue ticks
    /// (1e-4 fraction): `500` = 5% initial margin = 20x leverage,
    /// `1000` = 10% = 10x, etc. Valid range is `1..=10_000`
    /// (the upstream `MarginFractionTick` cap).
    ///
    /// Nautilus does not expose a `set_leverage` command on the execution
    /// trait, so this method is callable directly from strategy or bootstrap
    /// code.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing, the instrument is not
    /// registered, `initial_margin_fraction` is outside `1..=10_000`, or
    /// the dispatch pre-flight (nonce allocation, signing) fails. Transport
    /// errors after dispatch are logged but not returned synchronously.
    pub fn update_leverage(
        &self,
        instrument_id: InstrumentId,
        initial_margin_fraction: u16,
        margin_mode: LighterPositionMarginMode,
    ) -> anyhow::Result<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Lighter execution client cannot update leverage without credentials")
        })?;

        let market_index = self.registry.market_index(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("no Lighter market_index registered for instrument {instrument_id}")
        })?;

        anyhow::ensure!(
            (1..=10_000).contains(&initial_margin_fraction),
            "initial_margin_fraction must be in 1..=10_000, was {initial_margin_fraction}",
        );

        let ReservedTxContext {
            context,
            mut send_reservation,
        } = self.build_tx_context(credential)?;

        let captured_nonce = context.nonce;
        let captured_api_key_index = context.api_key_index;
        let mut rollback_guard =
            TxDispatchGuard::new(self.dispatch.clone(), credential, None, captured_nonce);
        let tx = UpdateLeverageTxInfo {
            context,
            market_index,
            initial_margin_fraction,
            margin_mode: margin_mode as u8,
            skip_nonce: 0,
        };

        let signed = sign_tx(
            &tx,
            lighter_chain_id(self.config.environment),
            &credential.private_key()?,
            fresh_k(),
        );
        let tx_info_str = TxInfoJson::update_leverage(&tx, &signed);
        let tx_info = serde_json::value::RawValue::from_string(tx_info_str)
            .context("failed to wrap signed Lighter update_leverage tx_info JSON")?;
        rollback_guard.disarm();
        let captured_tx_hash = signed.tx_hash_hex();

        let ws_client = self.ws_client.clone();
        let dispatch = self.dispatch.clone();
        let credential = credential.clone();
        let clock = self.clock;

        let tx_rate_limiter = self.tx_rate_limiter.clone();

        self.spawn_task("update_leverage", async move {
            send_reservation.wait_for_turn().await;
            await_tx_quota(&tx_rate_limiter).await;
            dispatch.enqueue_pending_sendtx(PendingSendTx {
                kind: PendingSendTxKind::Other,
                submitted_at: clock.get_time_ns(),
                nonce: captured_nonce,
                api_key_index: captured_api_key_index,
                tx_hash: captured_tx_hash,
            });

            if let Err(e) = ws_client
                .send_tx(LighterTxType::UpdateLeverage as u8, tx_info)
                .await
            {
                let reason = format!("Lighter update_leverage dispatch failed: {e}");
                log::error!("{reason} for {instrument_id}");
                dispatch.remove_pending_sendtx_by_nonce(captured_nonce);
                rollback_tx_dispatch(&dispatch, &credential, None, captured_nonce);
            }
            send_reservation.release();
            Ok(())
        });

        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthTokenRefreshOutcome {
    Rotated,
    Cancelled,
    Exhausted,
}

#[derive(Debug, Clone, Copy)]
struct AuthTokenRefreshBackoff {
    initial_delay: Duration,
    max_delay: Duration,
    window: Duration,
}

fn auth_token_rotation_channels(account_index: i64) -> [LighterWsChannel; 5] {
    [
        LighterWsChannel::AccountAllOrders(account_index),
        LighterWsChannel::AccountAllTrades(account_index),
        LighterWsChannel::AccountAllPositions(account_index),
        LighterWsChannel::AccountAllAssets(account_index),
        LighterWsChannel::UserStats(account_index),
    ]
}

async fn refresh_auth_token_until_rotated<MintToken, Subscribe, SubscribeFuture>(
    credential: &Credential,
    channels: &[LighterWsChannel],
    cancellation_token: &CancellationToken,
    backoff: AuthTokenRefreshBackoff,
    mut mint_token: MintToken,
    mut subscribe: Subscribe,
) -> AuthTokenRefreshOutcome
where
    MintToken: FnMut(&Credential) -> anyhow::Result<String>,
    Subscribe: FnMut(LighterWsChannel, String) -> SubscribeFuture,
    SubscribeFuture: Future<Output = Result<(), crate::websocket::error::LighterWsError>>,
{
    let retry_started = tokio::time::Instant::now();
    let mut retry_delay = backoff.initial_delay.min(backoff.max_delay);
    let mut attempt = 1_u32;

    loop {
        match rotate_auth_token_once(credential, channels, &mut mint_token, &mut subscribe).await {
            Ok(()) => {
                log::debug!(
                    "Lighter auth-token rotated for account_index={}, attempts={attempt}",
                    credential.account_index(),
                );
                return AuthTokenRefreshOutcome::Rotated;
            }
            Err(e) => {
                log::error!("Lighter auth-token rotation attempt {attempt} failed: {e:#}");
            }
        }

        let Some(remaining) = backoff.window.checked_sub(retry_started.elapsed()) else {
            log::error!(
                "Lighter auth-token rotation retry window exhausted: account_index={}, attempts={attempt}",
                credential.account_index(),
            );
            return AuthTokenRefreshOutcome::Exhausted;
        };

        if remaining.as_nanos() == 0 {
            log::error!(
                "Lighter auth-token rotation retry window exhausted: account_index={}, attempts={attempt}",
                credential.account_index(),
            );
            return AuthTokenRefreshOutcome::Exhausted;
        }

        let delay = retry_delay.min(remaining);
        log::warn!(
            "Retrying Lighter auth-token rotation in {:.3}s: account_index={}, attempts={attempt}",
            delay.as_secs_f64(),
            credential.account_index(),
        );

        if !sleep_or_auth_token_refresh_cancelled(delay, cancellation_token).await {
            return AuthTokenRefreshOutcome::Cancelled;
        }

        retry_delay = next_auth_token_refresh_retry_delay(retry_delay, backoff.max_delay);
        attempt = attempt.saturating_add(1);
    }
}

async fn rotate_auth_token_once<MintToken, Subscribe, SubscribeFuture>(
    credential: &Credential,
    channels: &[LighterWsChannel],
    mint_token: &mut MintToken,
    subscribe: &mut Subscribe,
) -> anyhow::Result<()>
where
    MintToken: FnMut(&Credential) -> anyhow::Result<String>,
    Subscribe: FnMut(LighterWsChannel, String) -> SubscribeFuture,
    SubscribeFuture: Future<Output = Result<(), crate::websocket::error::LighterWsError>>,
{
    let token =
        mint_token(credential).context("failed to mint Lighter auth token during rotation")?;
    let mut first_error = None;

    for channel in channels {
        if let Err(e) = subscribe(channel.clone(), token.clone()).await {
            log::error!("Lighter auth-token rotation: re-subscribe failed for {channel:?}: {e}",);
            first_error.get_or_insert_with(|| format!("{channel:?}: {e}"));
        }
    }

    if let Some(error) = first_error {
        anyhow::bail!("failed to re-subscribe Lighter account channels: {error}");
    }

    Ok(())
}

async fn sleep_or_auth_token_refresh_cancelled(
    duration: Duration,
    cancellation_token: &CancellationToken,
) -> bool {
    tokio::select! {
        () = cancellation_token.cancelled() => false,
        () = tokio::time::sleep(duration) => true,
    }
}

fn auth_token_refresh_next_delay(outcome: AuthTokenRefreshOutcome) -> Option<Duration> {
    match outcome {
        AuthTokenRefreshOutcome::Rotated => Some(AUTH_TOKEN_REFRESH_INTERVAL),
        AuthTokenRefreshOutcome::Cancelled => None,
        AuthTokenRefreshOutcome::Exhausted => Some(AUTH_TOKEN_REFRESH_RETRY_MAX_DELAY),
    }
}

fn next_auth_token_refresh_retry_delay(current: Duration, max: Duration) -> Duration {
    current.checked_mul(2).unwrap_or(max).min(max)
}

#[derive(Debug)]
struct ReservedTxContext {
    context: TxContext,
    send_reservation: TxSendReservation,
}

#[derive(Debug, Clone)]
struct TxSendSequencer {
    state: Arc<Mutex<TxSendSequencerState>>,
    version: Arc<AtomicU64>,
    changed: tokio::sync::watch::Sender<u64>,
}

impl TxSendSequencer {
    fn new() -> Self {
        let (changed, _) = tokio::sync::watch::channel(0);
        Self {
            state: Arc::new(Mutex::new(TxSendSequencerState::default())),
            version: Arc::new(AtomicU64::new(0)),
            changed,
        }
    }

    fn reserve(&self, account_index: i64, api_key_index: u8, nonce: i64) -> TxSendReservation {
        let key = TxSendKey {
            account_index,
            api_key_index,
        };
        self.state
            .lock()
            .expect(MUTEX_POISONED)
            .pending
            .entry(key)
            .or_default()
            .insert(nonce);
        self.notify_waiters();

        TxSendReservation {
            sequencer: self.clone(),
            key,
            nonce,
            released: false,
        }
    }

    async fn wait_for_turn(&self, key: TxSendKey, nonce: i64) {
        let mut changed = self.changed.subscribe();

        loop {
            if self.ready_to_send(key, nonce) {
                return;
            }

            if changed.changed().await.is_err() {
                tokio::task::yield_now().await;
            }
        }
    }

    fn release(&self, key: TxSendKey, nonce: i64) {
        let mut state = self.state.lock().expect(MUTEX_POISONED);
        let should_notify = if let Some(pending) = state.pending.get_mut(&key) {
            let removed = pending.remove(&nonce);
            if pending.is_empty() {
                state.pending.remove(&key);
            }
            removed
        } else {
            false
        };
        drop(state);

        if should_notify {
            self.notify_waiters();
        }
    }

    fn ready_to_send(&self, key: TxSendKey, nonce: i64) -> bool {
        let state = self.state.lock().expect(MUTEX_POISONED);
        state
            .pending
            .get(&key)
            .and_then(|pending| pending.first())
            .is_none_or(|first| *first >= nonce)
    }

    fn notify_waiters(&self) {
        let version = self.version.fetch_add(1, Ordering::AcqRel) + 1;
        let _ = self.changed.send(version);
    }
}

#[derive(Debug, Default)]
struct TxSendSequencerState {
    pending: BTreeMap<TxSendKey, BTreeSet<i64>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TxSendKey {
    account_index: i64,
    api_key_index: u8,
}

#[derive(Debug)]
struct TxSendReservation {
    sequencer: TxSendSequencer,
    key: TxSendKey,
    nonce: i64,
    released: bool,
}

impl TxSendReservation {
    async fn wait_for_turn(&self) {
        self.sequencer.wait_for_turn(self.key, self.nonce).await;
    }

    fn release(&mut self) {
        if self.released {
            return;
        }

        self.sequencer.release(self.key, self.nonce);
        self.released = true;
    }
}

impl Drop for TxSendReservation {
    fn drop(&mut self) {
        self.release();
    }
}

async fn wait_for_tx_send_reservations(reservations: &[&TxSendReservation]) {
    let Some(first) = reservations.first() else {
        return;
    };

    debug_assert!(
        reservations
            .iter()
            .all(|reservation| reservation.key == first.key),
        "batch send reservations must share one nonce stream",
    );

    let nonce = reservations
        .iter()
        .map(|reservation| reservation.nonce)
        .min()
        .expect("reservations is non-empty");
    first.sequencer.wait_for_turn(first.key, nonce).await;
}

fn release_prepared_create_reservations(prepared_orders: &mut [PreparedCreateOrder]) {
    for prepared in prepared_orders {
        prepared.send_reservation.release();
    }
}

fn release_prepared_cancel_reservations(prepared_cancels: &mut [PreparedCancelOrder]) {
    for prepared in prepared_cancels {
        prepared.send_reservation.release();
    }
}

struct PreparedCreateOrder {
    order: OrderAny,
    client_order_index: i64,
    tx_info: Box<serde_json::value::RawValue>,
    nonce: i64,
    api_key_index: u8,
    tx_hash: String,
    send_reservation: TxSendReservation,
}

struct PreparedCancelOrder {
    client_order_id: ClientOrderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    venue_order_id: Option<VenueOrderId>,
    tx_info: Box<serde_json::value::RawValue>,
    nonce: i64,
    api_key_index: u8,
    tx_hash: String,
    send_reservation: TxSendReservation,
}

struct PreparedModifyOrder {
    client_order_id: ClientOrderId,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    venue_order_id: Option<VenueOrderId>,
    tx_info: Box<serde_json::value::RawValue>,
    nonce: i64,
    api_key_index: u8,
    tx_hash: String,
    send_reservation: TxSendReservation,
}

struct PreparedIntegratorApproval {
    tx_info: String,
    nonce: i64,
    api_key_index: u8,
    approval_expiry: i64,
    send_reservation: TxSendReservation,
}

// Cross-check between a detected account tier and the configured REST quota:
// AboveTier when the override exceeds the tier limit, RaiseHint when the tier
// allows more than standard but no override is set.
#[derive(Debug, PartialEq, Eq)]
enum TierCrossCheck {
    AboveTier { documented: u32 },
    RaiseHint { documented: u32 },
}

// Computes the active REST quota to report and any cross-check advisory from the
// detected tier and the raw override. A zero override resolves to the standard
// default (matching resolve_quota), so the reported quota always matches the
// limiter. Pure so the reporting decision is unit-testable without log capture.
fn tier_quota_report(
    tier: LighterAccountTier,
    rest_quota_per_min: Option<u32>,
    standard_rest: u32,
) -> (u32, Option<TierCrossCheck>) {
    let configured = rest_quota_per_min.filter(|&n| n > 0);
    let active_rest = configured.unwrap_or(standard_rest);
    let cross_check = match (tier.documented_rest_quota_per_min(), configured) {
        (Some(documented), Some(configured)) if configured > documented => {
            Some(TierCrossCheck::AboveTier { documented })
        }
        (Some(documented), None) if documented > standard_rest => {
            Some(TierCrossCheck::RaiseHint { documented })
        }
        _ => None,
    };
    (active_rest, cross_check)
}

fn send_tx_batch_request(
    tx_types: &[u8],
    tx_infos: &[Box<serde_json::value::RawValue>],
) -> LighterSendTxBatchRequest {
    let tx_types =
        serde_json::to_string(tx_types).expect("tx_types JSON serialization cannot fail");
    let tx_infos: Vec<&str> = tx_infos.iter().map(|tx_info| tx_info.get()).collect();
    let tx_infos =
        serde_json::to_string(&tx_infos).expect("tx_infos JSON serialization cannot fail");

    LighterSendTxBatchRequest::new(tx_types, tx_infos)
}

struct TxDispatchGuard {
    dispatch: WsDispatchState,
    account_index: i64,
    api_key_index: u8,
    client_order_index: Option<i64>,
    client_order_id: Option<ClientOrderId>,
    nonce: i64,
    armed: bool,
}

impl TxDispatchGuard {
    fn new(
        dispatch: WsDispatchState,
        credential: &Credential,
        client_order_index: Option<i64>,
        nonce: i64,
    ) -> Self {
        Self {
            dispatch,
            account_index: credential.account_index(),
            api_key_index: credential.api_key_index(),
            client_order_index,
            client_order_id: None,
            nonce,
            armed: true,
        }
    }

    fn with_order_identity(mut self, client_order_id: ClientOrderId) -> Self {
        self.client_order_id = Some(client_order_id);
        self
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TxDispatchGuard {
    fn drop(&mut self) {
        if self.armed {
            rollback_tx_dispatch_indices(
                &self.dispatch,
                self.account_index,
                self.api_key_index,
                self.client_order_index,
                self.client_order_id.as_ref(),
                self.nonce,
            );
        }
    }
}

// SendTxAck: pop the pending entry and advance the nonce baseline (the venue
// applied the nonce); the account-orders frame drives the order lifecycle.
// The advance is a monotonic max, so a misattributed pop is harmless.
// SendTxAck: remove by echoed tx_hash when present, pop head only for
// hashless acks. An echoed hash is authoritative: on a miss the entry was
// already consumed or never enqueued, and a head fallback would attribute
// the ack to the wrong tx.
fn handle_send_tx_ack(
    dispatch: &WsDispatchState,
    account_index: Option<i64>,
    code: i64,
    tx_hash: Option<&str>,
) -> Option<PendingSendTx> {
    let popped = match tx_hash {
        Some(hash) => {
            let matched = dispatch.remove_pending_sendtx_by_hash(hash);
            if matched.is_none() {
                log::warn!("Lighter sendTx ack unmatched: tx_hash={hash} code={code}");
            }
            matched
        }
        None => dispatch.pop_pending_sendtx_head(),
    };

    if let (Some(pending), Some(account_index)) = (&popped, account_index) {
        let _ =
            dispatch
                .nonce_manager
                .ack_success(account_index, pending.api_key_index, pending.nonce);
    }

    log::debug!(
        "Lighter sendTx ack: code={code} tx_hash={tx_hash:?} popped_nonce={:?}",
        popped.as_ref().map(|p| p.nonce),
    );

    popped
}

fn spawn_acked_order_probe(pending: &PendingSendTx, context: AckedOrderProbeContext) {
    let Some(probe) = AckedOrderProbe::from_pending(pending) else {
        return;
    };

    get_runtime().spawn(async move {
        tokio::select! {
            () = context.cancellation_token.cancelled() => return,
            () = tokio::time::sleep(ACKED_ORDER_LOOKUP_DELAY) => {}
        }

        if let Err(e) = probe_acked_order(probe, &context).await {
            log::warn!("Lighter acked order no-op probe failed: {e:?}");
        }
    });
}

#[derive(Clone)]
struct AckedOrderProbeContext {
    http_client: LighterHttpClient,
    registry: Arc<MarketRegistry>,
    credential: Credential,
    dispatch: WsDispatchState,
    emitter: ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    cancellation_token: CancellationToken,
}

async fn probe_acked_order(
    probe: AckedOrderProbe,
    context: &AckedOrderProbeContext,
) -> anyhow::Result<()> {
    let report = lookup_order_status_report(
        &context.http_client,
        &context.registry,
        &context.credential,
        context.account_id,
        Some(probe.instrument_id()),
        Some(&probe.client_order_id()),
        probe.venue_order_id().as_ref(),
        &context.dispatch,
        context.clock,
    )
    .await?;

    if let Some(report) = &report {
        context.dispatch.seed_accepted_from_report(report);
    }

    emit_ack_noop_rejection_if_missing(
        &probe,
        report.is_some(),
        &context.emitter,
        context.clock.get_time_ns(),
    );
    Ok(())
}

fn emit_ack_noop_rejection_if_missing(
    probe: &AckedOrderProbe,
    order_found: bool,
    emitter: &ExecutionEventEmitter,
    now: UnixNanos,
) -> bool {
    if order_found {
        return false;
    }

    match probe {
        AckedOrderProbe::Cancel {
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
        } => {
            let reason = "Lighter cancel_order no-op: order not found after venue ack";
            log::warn!("{reason} for {client_order_id}");
            emitter.emit_order_cancel_rejected_event(
                *strategy_id,
                *instrument_id,
                *client_order_id,
                *venue_order_id,
                reason,
                now,
            );
        }
        AckedOrderProbe::Modify {
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
        } => {
            let reason = "Lighter modify_order no-op: order not found after venue ack";
            log::warn!("{reason} for {client_order_id}");
            emitter.emit_order_modify_rejected_event(
                *strategy_id,
                *instrument_id,
                *client_order_id,
                *venue_order_id,
                reason,
                now,
            );
        }
    }

    true
}

#[derive(Debug, Clone)]
enum AckedOrderProbe {
    Cancel {
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    },
    Modify {
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    },
}

impl AckedOrderProbe {
    fn from_pending(pending: &PendingSendTx) -> Option<Self> {
        match &pending.kind {
            PendingSendTxKind::Cancel {
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
            } => Some(Self::Cancel {
                strategy_id: *strategy_id,
                instrument_id: *instrument_id,
                client_order_id: *client_order_id,
                venue_order_id: *venue_order_id,
            }),
            PendingSendTxKind::Modify {
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
            } => Some(Self::Modify {
                strategy_id: *strategy_id,
                instrument_id: *instrument_id,
                client_order_id: *client_order_id,
                venue_order_id: *venue_order_id,
            }),
            PendingSendTxKind::Create { .. } | PendingSendTxKind::Other => None,
        }
    }

    fn instrument_id(&self) -> InstrumentId {
        match self {
            Self::Cancel { instrument_id, .. } | Self::Modify { instrument_id, .. } => {
                *instrument_id
            }
        }
    }

    fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Cancel {
                client_order_id, ..
            }
            | Self::Modify {
                client_order_id, ..
            } => *client_order_id,
        }
    }

    fn venue_order_id(&self) -> Option<VenueOrderId> {
        match self {
            Self::Cancel { venue_order_id, .. } | Self::Modify { venue_order_id, .. } => {
                *venue_order_id
            }
        }
    }
}

// SendTxRejected: attribute by echoed tx_hash when present (authoritative,
// no head fallback on a miss); otherwise pop head for Ack or
// head-within-window for BareError. Create emits OrderRejected, cancel/modify
// emit their typed rejections, and Other recovers via reconciliation. All
// attributed rejections roll the nonce back when still the latest issuance.
// Returns true on an invalid-nonce code: the sequential stream is wedged and
// needs a hard refresh.
#[expect(
    clippy::too_many_arguments,
    reason = "consumer-loop sink that flattens one SendTxRejected message without a wrapper struct"
)]
fn handle_send_tx_rejection(
    dispatch: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    account_index: Option<i64>,
    now: UnixNanos,
    source: SendTxRejectionSource,
    code: Option<i64>,
    message: &str,
    tx_hash: Option<&str>,
) -> bool {
    let needs_nonce_resync = code == Some(LIGHTER_ERROR_CODE_INVALID_NONCE);

    let pending = match tx_hash {
        Some(hash) => dispatch.remove_pending_sendtx_by_hash(hash),
        None => match source {
            SendTxRejectionSource::Ack => dispatch.pop_pending_sendtx_head(),
            SendTxRejectionSource::BareError => {
                dispatch.pop_pending_sendtx_within(now, SENDTX_BARE_ERROR_WINDOW_MS)
            }
        },
    };
    let Some(pending) = pending else {
        log::warn!(
            "Lighter sendTx rejection unattributed (source={source:?} code={code:?}): {message}",
        );
        return needs_nonce_resync;
    };

    let reason = format!(
        "Lighter venue rejected sendTx (code={}): {message}",
        code.map_or_else(|| "?".into(), |c| c.to_string()),
    );

    match &pending.kind {
        PendingSendTxKind::Create {
            order,
            client_order_index,
        } => {
            let cloid = order.client_order_id();
            log::error!(
                "{reason} attributed to cloid={cloid} nonce={} api_key_index={}",
                pending.nonce,
                pending.api_key_index,
            );

            if let Some(account_index) = account_index {
                let _ = dispatch.nonce_manager.ack_failure_if_latest(
                    account_index,
                    pending.api_key_index,
                    pending.nonce,
                );
            }
            dispatch.forget_cloid(*client_order_index);
            dispatch.forget_order_identity(&cloid);
            emitter.emit_order_rejected(
                order,
                &reason,
                now,
                lighter_reason_indicates_post_only_rejection(message),
            );
        }
        PendingSendTxKind::Cancel {
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
        } => {
            log::error!(
                "{reason} attributed to cancel cloid={client_order_id} nonce={} api_key_index={}",
                pending.nonce,
                pending.api_key_index,
            );

            if let Some(account_index) = account_index {
                let _ = dispatch.nonce_manager.ack_failure_if_latest(
                    account_index,
                    pending.api_key_index,
                    pending.nonce,
                );
            }
            emitter.emit_order_cancel_rejected_event(
                *strategy_id,
                *instrument_id,
                *client_order_id,
                *venue_order_id,
                &reason,
                now,
            );
        }
        PendingSendTxKind::Modify {
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
        } => {
            log::error!(
                "{reason} attributed to modify cloid={client_order_id} nonce={} api_key_index={}",
                pending.nonce,
                pending.api_key_index,
            );

            if let Some(account_index) = account_index {
                let _ = dispatch.nonce_manager.ack_failure_if_latest(
                    account_index,
                    pending.api_key_index,
                    pending.nonce,
                );
            }
            emitter.emit_order_modify_rejected_event(
                *strategy_id,
                *instrument_id,
                *client_order_id,
                *venue_order_id,
                &reason,
                now,
            );
        }
        PendingSendTxKind::Other => {
            if let Some(account_index) = account_index {
                let _ = dispatch.nonce_manager.ack_failure_if_latest(
                    account_index,
                    pending.api_key_index,
                    pending.nonce,
                );
            }
            log::warn!(
                "{reason} on non-create sendTx (nonce={} api_key_index={})",
                pending.nonce,
                pending.api_key_index,
            );
        }
    }

    needs_nonce_resync
}

fn lighter_reason_indicates_post_only_rejection(reason: &str) -> bool {
    let normalized: String = reason
        .chars()
        .filter_map(|ch| {
            if ch == '-' || ch == '_' || ch.is_whitespace() {
                None
            } else {
                Some(ch.to_ascii_lowercase())
            }
        })
        .collect();

    normalized.contains("postonly") || normalized.contains("postwouldexecute")
}

// A sendTxBatch success covers every tx (single result code). Batch txs
// produce no WS acks, so this is the only baseline signal for batch flows.
fn advance_baseline_for_batch(
    dispatch: &WsDispatchState,
    credential: &Credential,
    nonces: impl Iterator<Item = i64>,
) {
    if let Some(max_nonce) = nonces.max() {
        let _ = dispatch.nonce_manager.ack_success(
            credential.account_index(),
            credential.api_key_index(),
            max_nonce,
        );
    }
}

// Batch rejections surface as `LighterHttpError::Venue`, not WS frames, so
// the consumer-loop resync never sees them; realign allocation here so a
// wedged batch flow recovers without a reconnect.
async fn resync_nonce_after_invalid_nonce(
    http_client: &LighterHttpClient,
    dispatch: &WsDispatchState,
    credential: &Credential,
    error: &LighterHttpError,
) {
    if !matches!(
        error,
        LighterHttpError::Venue { code, .. } if *code == LIGHTER_ERROR_CODE_INVALID_NONCE
    ) {
        return;
    }

    match http_client
        .get_next_nonce(credential.account_index(), credential.api_key_index())
        .await
    {
        Ok(response) => {
            dispatch.nonce_manager.refresh(
                credential.account_index(),
                credential.api_key_index(),
                response.nonce,
            );
            log::info!(
                "Hard-refreshed Lighter nonce after invalid-nonce batch rejection: \
                 account_index={}, next_nonce={}",
                credential.account_index(),
                response.nonce,
            );
        }
        Err(e) => {
            log::error!("Failed to refresh Lighter nonce after invalid-nonce batch rejection: {e}");
        }
    }
}

fn rollback_tx_dispatch(
    dispatch: &WsDispatchState,
    credential: &Credential,
    client_order_index: Option<i64>,
    nonce: i64,
) {
    rollback_tx_dispatch_indices(
        dispatch,
        credential.account_index(),
        credential.api_key_index(),
        client_order_index,
        None,
        nonce,
    );
}

fn rollback_tx_dispatch_create(
    dispatch: &WsDispatchState,
    credential: &Credential,
    client_order_index: Option<i64>,
    client_order_id: &ClientOrderId,
    nonce: i64,
) {
    rollback_tx_dispatch_indices(
        dispatch,
        credential.account_index(),
        credential.api_key_index(),
        client_order_index,
        Some(client_order_id),
        nonce,
    );
}

// Roll back only while still the latest issuance: decrementing past a newer
// signed tx would duplicate its nonce on the wire. Skipped rollbacks heal
// via the baseline advance or venue resync.
fn rollback_tx_dispatch_indices(
    dispatch: &WsDispatchState,
    account_index: i64,
    api_key_index: u8,
    client_order_index: Option<i64>,
    client_order_id: Option<&ClientOrderId>,
    nonce: i64,
) {
    let _ = dispatch
        .nonce_manager
        .ack_failure_if_latest(account_index, api_key_index, nonce);

    if let Some(client_order_index) = client_order_index {
        dispatch.forget_cloid(client_order_index);
    }

    if let Some(cloid) = client_order_id {
        dispatch.forget_order_identity(cloid);
    }
}

fn integrator_attributes() -> L2TxAttributes {
    L2TxAttributes {
        integrator_account_index: LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
        integrator_taker_fee: 0,
        integrator_maker_fee: 0,
        skip_nonce: 0,
    }
}

/// Format a `start_ms,end_ms` window for Lighter's `between_timestamps`
/// query parameter. Returns `None` when neither bound is set; an unset end
/// defaults to the current time so the venue scopes pagination to the
/// half-open window.
fn format_between_timestamps(
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
    ts_now: UnixNanos,
) -> Option<String> {
    let (start, end) = match (start, end) {
        (None, None) => return None,
        (Some(s), Some(e)) => (s, e),
        (Some(s), None) => (s, ts_now),
        (None, Some(e)) => (UnixNanos::from(0), e),
    };
    let start_ms = start.as_u64() / 1_000_000;
    let end_ms = end.as_u64() / 1_000_000;
    Some(format!("{start_ms},{end_ms}"))
}

#[async_trait(?Send)]
impl ExecutionClient for LighterExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *LIGHTER_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started Lighter execution client: client_id={}, account_id={}, environment={:?}, has_credentials={}",
            self.core.client_id,
            self.core.account_id,
            self.config.environment,
            self.has_credentials(),
        );

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        log::info!("Stopping Lighter execution client {}", self.core.client_id);

        self.cancellation_token.cancel();

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();

        self.core.set_disconnected();
        self.core.set_stopped();

        log::info!("Lighter execution client stopped");
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Without credentials the engine would accept the connection and
        // then deny every order per-submission. Fail before any WS/REST
        // work so reconciliation and strategies never start.
        if !self.has_credentials() {
            anyhow::bail!(
                "Lighter execution client requires credentials; \
                 set private_key, account_index, and api_key_index in the config \
                 (or the LIGHTER_{{MAINNET,TESTNET}}_* environment variables)"
            );
        }

        log::info!(
            "Connecting Lighter execution client {}",
            self.core.client_id
        );

        // Rotate the cancellation token before reconnect so a previous stop()
        // does not signal the new consumer task to exit immediately.
        if self.cancellation_token.is_cancelled() {
            self.cancellation_token = CancellationToken::new();
        }

        // Reset the readiness gate and clear derived position/account caches
        // so a prior session's state cannot leak past the strict-await gate.
        // The Reconnected path (WS-layer transparent reconnect) is unaffected:
        // it does not re-enter `connect()`. Its next `account_all_positions`
        // frame replaces the position cache through the consumption loop.
        self.dispatch.account_streams_ready.reset();
        self.dispatch.clear_position_cache();
        self.dispatch.clear_account_state_cache();

        self.ensure_instruments_initialized_async().await?;
        self.refresh_nonce().await?;
        self.detect_account_tier().await;

        if let Err(e) = self.submit_integrator_auto_approval().await {
            // Bail on venue 21149 ("integrator is not approved") so the
            // operator catches it at startup rather than at first order.
            // Other failures are tolerated: approval is account-scoped and
            // may already be in place, or a reconnect can retry.
            let is_unapproved = e.chain().any(|cause| {
                matches!(
                    cause.downcast_ref::<LighterHttpError>(),
                    Some(LighterHttpError::Venue { code: 21149, .. }),
                )
            });

            if is_unapproved {
                return Err(e.context(
                    "Lighter account is not integrator-approved (venue 21149); \
                     orders cannot be placed",
                ));
            }
            log::error!("Lighter integrator approval failed; continuing startup: {e:?}");
        }

        if let Err(e) = self.refresh_nonce().await {
            log::debug!(
                "Failed to refresh Lighter nonce after integrator approval; continuing startup: {e:?}"
            );
        }
        self.spawn_ws_consumer().await?;

        if let Err(e) = self.await_account_streams_ready(30.0).await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
            self.cancellation_token.cancel();

            if let Err(disconnect_err) = self.ws_client.disconnect().await {
                log::error!(
                    "Error disconnecting Lighter WebSocket during connect teardown: {disconnect_err}"
                );
            }

            // Await the consumer task to completion before returning so a
            // queued marker from the failed session cannot call `mark_*`
            // on the shared readiness handle after a caller's retry has
            // reset it. On timeout we still abort and drain the handle to
            // completion rather than detaching, so the task is provably
            // dead before this function returns.
            let taken_handle = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take();
            if let Some(handle) = taken_handle {
                let abort_handle = handle.abort_handle();
                let mut handle = Box::pin(handle);
                tokio::select! {
                    join_res = &mut handle => match join_res {
                        Ok(()) => log::debug!(
                            "Lighter execution consumer task completed during connect teardown"
                        ),
                        Err(join_err) if join_err.is_cancelled() => log::debug!(
                            "Lighter execution consumer task cancelled during connect teardown"
                        ),
                        Err(join_err) => log::error!(
                            "Lighter execution consumer task error during connect teardown: {join_err}"
                        ),
                    },
                    () = tokio::time::sleep(WS_CONSUMER_SHUTDOWN_TIMEOUT) => {
                        log::warn!(
                            "Timeout waiting for Lighter execution consumer during connect teardown, aborting",
                        );
                        abort_handle.abort();
                        let _ = handle.await;
                    }
                }
            }

            self.abort_pending_tasks();
            return Err(e);
        }

        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        log::info!(
            "Disconnecting Lighter execution client {}",
            self.core.client_id
        );

        // Signal the consumption loop to drain.
        self.cancellation_token.cancel();

        if let Err(e) = self.ws_client.disconnect().await {
            log::warn!("Error disconnecting Lighter WebSocket client: {e}");
        }

        let ws_stream_handle = { self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() };

        if let Some(handle) = ws_stream_handle {
            let abort_handle = handle.abort_handle();
            match tokio::time::timeout(WS_CONSUMER_SHUTDOWN_TIMEOUT, handle).await {
                Ok(Ok(())) => log::debug!("Lighter execution consumer task completed"),
                Ok(Err(e)) if e.is_cancelled() => {
                    log::debug!("Lighter execution consumer task cancelled");
                }
                Ok(Err(e)) => log::error!("Lighter execution consumer task error: {e}"),
                Err(_) => {
                    log::warn!("Timeout waiting for Lighter execution consumer task, aborting");
                    abort_handle.abort();
                }
            }
        }

        self.abort_pending_tasks();

        self.core.set_disconnected();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Lighter execution client cannot submit without credentials")
        })?;

        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let cached_instrument = self
            .core
            .cache()
            .instrument(&order.instrument_id())
            .cloned();

        if let Some(reason) = local_submit_denial_reason(&order, cached_instrument.as_ref()) {
            self.emitter.emit_order_denied(&order, &reason);
            return Ok(());
        }

        let slippage_bps = self.resolve_slippage_bps(cmd.params.as_ref());
        if let Err(e) = self.dispatch_signed_create_order(&order, credential, slippage_bps) {
            self.emitter
                .emit_order_denied(&order, &format!("Lighter submit_order failed: {e}"));
        }

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Lighter execution client cannot submit without credentials")
        })?;

        if cmd.order_list.client_order_ids.is_empty() {
            log::debug!("submit_order_list called with empty order list");
            return Ok(());
        }

        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        if orders.len() > LIGHTER_MAX_BATCH_TX {
            let reason = format!(
                "Lighter sendTxBatch supports at most {LIGHTER_MAX_BATCH_TX} txs, was {}",
                orders.len(),
            );

            for order in &orders {
                self.emitter.emit_order_denied(order, &reason);
            }
            return Ok(());
        }

        if orders.iter().any(is_grouped_order) {
            let reason = format!(
                "Lighter submit_order_list supports only independent orders; \
                 grouped contingency lists remain out of scope (order_list_id={})",
                cmd.order_list.id,
            );

            for order in &orders {
                self.emitter.emit_order_denied(order, &reason);
            }
            return Ok(());
        }

        let slippage_bps = self.resolve_slippage_bps(cmd.params.as_ref());
        let mut prepared_orders = Vec::with_capacity(orders.len());

        for order in orders {
            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                continue;
            }

            let cached_instrument = self
                .core
                .cache()
                .instrument(&order.instrument_id())
                .cloned();

            if let Some(reason) = local_submit_denial_reason(&order, cached_instrument.as_ref()) {
                self.emitter.emit_order_denied(&order, &reason);
                continue;
            }

            match self.prepare_signed_create_order(&order, credential, slippage_bps) {
                Ok(prepared) => prepared_orders.push(prepared),
                Err(e) => {
                    let reason = format!("Lighter submit_order_list failed: {e}");

                    self.emitter.emit_order_denied(&order, &reason);
                }
            }
        }

        if prepared_orders.is_empty() {
            log::warn!(
                "Lighter submit_order_list: no supported orders to dispatch for {}",
                cmd.order_list.id,
            );
            return Ok(());
        }

        for prepared in &prepared_orders {
            self.emitter.emit_order_submitted(&prepared.order);
        }

        let tx_types = vec![LighterTxType::CreateOrder as u8; prepared_orders.len()];
        let tx_infos: Vec<Box<serde_json::value::RawValue>> = prepared_orders
            .iter()
            .map(|prepared| prepared.tx_info.clone())
            .collect();
        let request = send_tx_batch_request(&tx_types, &tx_infos);
        let http_client = self.http_client.clone();
        let dispatch = self.dispatch.clone();
        let credential = credential.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("submit_order_list", async move {
            let mut prepared_orders = prepared_orders;
            log::debug!(
                "Lighter submit_order_list: queueing {} CreateOrder txs",
                prepared_orders.len(),
            );

            let reservations = prepared_orders
                .iter()
                .map(|prepared| &prepared.send_reservation)
                .collect::<Vec<_>>();
            wait_for_tx_send_reservations(&reservations).await;

            match http_client.send_tx_batch(&request).await {
                Ok(_) => {
                    advance_baseline_for_batch(
                        &dispatch,
                        &credential,
                        prepared_orders.iter().map(|prepared| prepared.nonce),
                    );
                }
                Err(e) => {
                    let reason = format!("Lighter submit_order_list dispatch failed: {e}");
                    log::error!("{reason}");

                    // Reverse order so each rollback is the then-latest nonce
                    for prepared in prepared_orders.iter().rev() {
                        let client_order_id = prepared.order.client_order_id();
                        rollback_tx_dispatch_create(
                            &dispatch,
                            &credential,
                            Some(prepared.client_order_index),
                            &client_order_id,
                            prepared.nonce,
                        );

                        emitter.emit_order_rejected(
                            &prepared.order,
                            &reason,
                            clock.get_time_ns(),
                            false,
                        );
                    }

                    resync_nonce_after_invalid_nonce(&http_client, &dispatch, &credential, &e)
                        .await;
                }
            }
            release_prepared_create_reservations(&mut prepared_orders);
            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Lighter execution client cannot modify without credentials")
        })?;
        self.dispatch_signed_modify_order(&cmd, credential);
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Lighter execution client cannot cancel without credentials")
        })?;
        self.dispatch_signed_cancel_order(&cmd, credential);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        // Iterate over open orders for the instrument and cancel each. The
        // venue offers a `CancelAllOrders` tx but it spans the whole account
        // rather than a single market; doing per-order cancels keeps scope
        // tight and avoids cancelling positions in unrelated markets.
        let cache = self.core.cache();
        let open_orders: Vec<ClientOrderId> = cache
            .orders_open(None, Some(&cmd.instrument_id), None, None, None)
            .into_iter()
            .map(|o| o.client_order_id())
            .collect();

        for client_order_id in open_orders {
            let order_cmd = cancel_order_from_cancel_all(&cmd, client_order_id);

            if let Err(e) = self.cancel_order(order_cmd) {
                log::warn!("cancel_all_orders: cancel for {client_order_id} failed: {e}");
            }
        }
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Lighter execution client cannot cancel without credentials")
        })?;

        if cmd.cancels.is_empty() {
            log::debug!("batch_cancel_orders called with empty cancel list");
            return Ok(());
        }

        if cmd.cancels.len() > LIGHTER_MAX_BATCH_TX {
            let reason = format!(
                "Lighter sendTxBatch supports at most {LIGHTER_MAX_BATCH_TX} txs, was {}",
                cmd.cancels.len(),
            );

            for cancel in &cmd.cancels {
                self.emitter.emit_order_cancel_rejected_event(
                    cancel.strategy_id,
                    cancel.instrument_id,
                    cancel.client_order_id,
                    cancel.venue_order_id,
                    &reason,
                    self.clock.get_time_ns(),
                );
            }
            return Ok(());
        }

        let mut prepared_cancels = Vec::with_capacity(cmd.cancels.len());
        for cancel in &cmd.cancels {
            match self.prepare_signed_cancel_order(cancel, credential) {
                Ok(prepared) => prepared_cancels.push(prepared),
                Err(e) => {
                    let reason = format!("Lighter batch_cancel_orders failed: {e}");
                    log::warn!("{reason} for {}", cancel.client_order_id);

                    self.emitter.emit_order_cancel_rejected_event(
                        cancel.strategy_id,
                        cancel.instrument_id,
                        cancel.client_order_id,
                        cancel.venue_order_id,
                        &reason,
                        self.clock.get_time_ns(),
                    );
                }
            }
        }

        if prepared_cancels.is_empty() {
            log::warn!("Lighter batch_cancel_orders: no cancellable orders to dispatch");
            return Ok(());
        }

        let tx_types = vec![LighterTxType::CancelOrder as u8; prepared_cancels.len()];
        let tx_infos: Vec<Box<serde_json::value::RawValue>> = prepared_cancels
            .iter()
            .map(|prepared| prepared.tx_info.clone())
            .collect();
        let request = send_tx_batch_request(&tx_types, &tx_infos);
        let http_client = self.http_client.clone();
        let dispatch = self.dispatch.clone();
        let credential = credential.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("batch_cancel_orders", async move {
            let mut prepared_cancels = prepared_cancels;
            log::debug!(
                "Lighter batch_cancel_orders: queueing {} CancelOrder txs",
                prepared_cancels.len(),
            );

            let reservations = prepared_cancels
                .iter()
                .map(|prepared| &prepared.send_reservation)
                .collect::<Vec<_>>();
            wait_for_tx_send_reservations(&reservations).await;

            match http_client.send_tx_batch(&request).await {
                Ok(_) => {
                    advance_baseline_for_batch(
                        &dispatch,
                        &credential,
                        prepared_cancels.iter().map(|prepared| prepared.nonce),
                    );
                }
                Err(e) => {
                    let reason = format!("Lighter batch_cancel_orders dispatch failed: {e}");
                    log::error!("{reason}");

                    // Reverse order so each rollback is the then-latest nonce
                    for prepared in prepared_cancels.iter().rev() {
                        rollback_tx_dispatch(&dispatch, &credential, None, prepared.nonce);

                        emitter.emit_order_cancel_rejected_event(
                            prepared.strategy_id,
                            prepared.instrument_id,
                            prepared.client_order_id,
                            prepared.venue_order_id,
                            &reason,
                            clock.get_time_ns(),
                        );
                    }

                    resync_nonce_after_invalid_nonce(&http_client, &dispatch, &credential, &e)
                        .await;
                }
            }
            release_prepared_cancel_reservations(&mut prepared_cancels);
            Ok(())
        });

        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        // Lighter has no public REST endpoint that returns a snapshot of
        // account balances and margins; the only authoritative source is the
        // `account_all_assets` WebSocket stream. Replay the most recent
        // cached state so the engine sees something synchronously. The
        // cache is populated by the consumption loop on every venue push.
        let cached = self.dispatch.snapshot_account_state();
        match cached {
            Some(state) => {
                log::debug!("Lighter query_account replaying cached AccountState");
                self.emitter.send_account_state(state);
            }
            None => {
                log::warn!(
                    "Lighter query_account: no AccountState cached yet \
                     (account_all_assets stream has not pushed since connect)",
                );
            }
        }
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let credential = self
            .credential
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Lighter query_order requires credentials"))?
            .clone();
        let registry = Arc::clone(&self.registry);
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let core_account_id = self.core.account_id;
        let dispatch = self.dispatch.clone();
        let clock = self.clock;

        self.spawn_task("query_order", async move {
            let report = lookup_order_status_report(
                &http_client,
                &registry,
                &credential,
                core_account_id,
                Some(cmd.instrument_id),
                Some(&cmd.client_order_id),
                cmd.venue_order_id.as_ref(),
                &dispatch,
                clock,
            )
            .await?;

            match report {
                Some(report) => {
                    log::debug!(
                        "Lighter query_order returning report for {}",
                        cmd.client_order_id
                    );
                    dispatch.seed_accepted_from_report(&report);
                    emitter.send_order_status_report(report);
                }
                None => {
                    log::warn!(
                        "Lighter query_order: no order found for {}",
                        cmd.client_order_id,
                    );
                }
            }
            Ok(())
        });
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(credential) = &self.credential else {
            log::warn!("Lighter generate_order_status_report: no credentials");
            return Ok(None);
        };

        if cmd.client_order_id.is_none() && cmd.venue_order_id.is_none() {
            log::warn!(
                "Lighter generate_order_status_report: must supply client_order_id or venue_order_id",
            );
            return Ok(None);
        }
        let report = lookup_order_status_report(
            &self.http_client,
            &self.registry,
            credential,
            self.core.account_id,
            cmd.instrument_id,
            cmd.client_order_id.as_ref(),
            cmd.venue_order_id.as_ref(),
            &self.dispatch,
            self.clock,
        )
        .await?;

        if let Some(report) = &report {
            self.dispatch.seed_accepted_from_report(report);
        }

        Ok(report)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let Some(credential) = &self.credential else {
            log::warn!("Lighter generate_order_status_reports: no credentials");
            return Ok(Vec::new());
        };

        let auth = build_auth_token_for(credential)
            .context("failed to mint Lighter auth token for report fetch")?;
        let ts_init = self.clock.get_time_ns();

        // Lighter exposes accountActiveOrders only per-market. Mass-status
        // requests with no scope iterate over account-active markets rather
        // than fanning out to every registered market, since the venue's REST
        // rate limit (60 req/min) would make a 180-market fan-out take
        // minutes. Account streams seed this set from live order, trade, and
        // position frames; if startup reconciliation reaches this path before
        // any market is known, one unscoped inactive-order page walk seeds it
        // from historical account activity.
        if cmd.instrument_id.is_none() && self.dispatch.active_markets_snapshot().is_empty() {
            seed_active_markets_from_inactive_orders(
                &self.http_client,
                &self.dispatch,
                credential,
                &auth,
                format_between_timestamps(cmd.start, cmd.end, ts_init),
            )
            .await;
        }

        let market_indices = match cmd.instrument_id {
            Some(id) => match self.registry.market_index(&id) {
                Some(idx) => vec![idx],
                None => {
                    log::warn!(
                        "Lighter generate_order_status_reports: market_index unknown for {id}",
                    );
                    return Ok(Vec::new());
                }
            },
            None => self.dispatch.active_markets_snapshot(),
        };

        if market_indices.is_empty() {
            log::debug!(
                "Lighter generate_order_status_reports: no active markets yet; returning empty",
            );
            return Ok(Vec::new());
        }

        let mut active_reports: Vec<OrderStatusReport> = Vec::new();
        let mut inactive_reports: Vec<OrderStatusReport> = Vec::new();

        // Active orders are by definition still open. Returning them
        // unconditionally even when `cmd.start` is set: an open order's
        // last activity can predate the lookback window without changing
        // the fact that the order is currently live and reconciliation
        // needs to know about it.
        for market_index in market_indices {
            let active = match self
                .http_client
                .get_account_active_orders(&LighterAccountActiveOrdersQuery {
                    authorization: None,
                    auth: Some(auth.clone()),
                    account_index: credential.account_index(),
                    market_id: market_index,
                })
                .await
            {
                Ok(response) => response,
                Err(e) => {
                    log::warn!(
                        "Lighter active orders fetch failed for market_index={market_index}: {}",
                        scrub_auth(&format!("{e:#}")),
                    );
                    continue;
                }
            };

            for order in &active.orders {
                self.dispatch.note_active_market(order.market_index);
                if let Some(report) = parse_http_order_to_report(
                    order,
                    &self.registry,
                    self.core.account_id,
                    ts_init,
                    &self.dispatch.cloid_map,
                ) {
                    active_reports.push(report);
                }
            }
        }

        // Inactive orders (filled / canceled) are required when the engine
        // asks for non-`open_only` reports during a wider reconciliation.
        // Pagination is followed because a single market can hold more than
        // 200 historical inactive orders for a long-running account. The
        // venue-side `between_timestamps` window is set when `cmd.start`
        // / `cmd.end` are present so the venue, not the client, scopes the
        // pagination: important under the 60 req/min REST quota.
        if !cmd.open_only {
            let inactive_markets: Vec<i16> = match cmd.instrument_id {
                Some(id) => self
                    .registry
                    .market_index(&id)
                    .map(|m| vec![m])
                    .unwrap_or_default(),
                None => self.dispatch.active_markets_snapshot(),
            };

            let between_timestamps = format_between_timestamps(cmd.start, cmd.end, ts_init);

            for market_id in inactive_markets {
                let mut cursor: Option<String> = None;

                loop {
                    match self
                        .http_client
                        .get_account_inactive_orders(&LighterAccountInactiveOrdersQuery {
                            authorization: None,
                            auth: Some(auth.clone()),
                            account_index: credential.account_index(),
                            market_id: Some(market_id),
                            ask_filter: None,
                            between_timestamps: between_timestamps.clone(),
                            cursor: cursor.clone(),
                            limit: LIGHTER_REST_PAGE_SIZE,
                        })
                        .await
                    {
                        Ok(inactive) => {
                            for order in &inactive.orders {
                                self.dispatch.note_active_market(order.market_index);
                                if let Some(report) = parse_http_order_to_report(
                                    order,
                                    &self.registry,
                                    self.core.account_id,
                                    ts_init,
                                    &self.dispatch.cloid_map,
                                ) {
                                    inactive_reports.push(report);
                                }
                            }

                            match inactive.next_cursor {
                                Some(next) if !next.is_empty() => cursor = Some(next),
                                _ => break,
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "Lighter inactive orders fetch failed for market_index={market_id}: {}",
                                scrub_auth(&format!("{e:#}")),
                            );
                            break;
                        }
                    }
                }
            }
        }

        // Apply start/end only to inactive reports. Active reports are
        // always current and the engine needs them regardless of lookback.
        let inactive_reports: Vec<OrderStatusReport> = match (cmd.start, cmd.end) {
            (Some(start), Some(end)) => inactive_reports
                .into_iter()
                .filter(|r| r.ts_last >= start && r.ts_last <= end)
                .collect(),
            (Some(start), None) => inactive_reports
                .into_iter()
                .filter(|r| r.ts_last >= start)
                .collect(),
            (None, Some(end)) => inactive_reports
                .into_iter()
                .filter(|r| r.ts_last <= end)
                .collect(),
            (None, None) => inactive_reports,
        };

        let mut reports = active_reports;
        reports.extend(inactive_reports);

        for report in &reports {
            self.dispatch.seed_accepted_from_report(report);
        }

        log::debug!("Generated {} Lighter order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let Some(credential) = &self.credential else {
            log::warn!("Lighter generate_fill_reports: no credentials");
            return Ok(Vec::new());
        };

        let market_id = cmd
            .instrument_id
            .and_then(|id| self.registry.market_index(&id));

        let auth = build_auth_token_for(credential)
            .context("failed to mint Lighter auth token for fill fetch")?;

        let ts_init = self.clock.get_time_ns();
        let mut reports = Vec::new();
        let mut cursor: Option<String> = None;

        loop {
            let query = LighterTradesQuery {
                authorization: None,
                auth: Some(auth.clone()),
                market_id,
                account_index: Some(credential.account_index()),
                order_index: None,
                sort_by: LighterTradeSortBy::Timestamp,
                sort_dir: Some(LighterSortDirection::Desc),
                cursor: cursor.clone(),
                from_timestamp: cmd.start.map(|ts| (ts.as_u64() / 1_000_000) as i64),
                ask_filter: None,
                role: None,
                trade_type: None,
                limit: LIGHTER_REST_PAGE_SIZE,
                aggregate: None,
            };

            let response = match self.http_client.get_trades(&query).await {
                Ok(response) => response,
                Err(e) => {
                    // `{e:#}` preserves the venue's status/body across the
                    // outer context wrap; `scrub_auth` masks any `auth=`
                    // query value the HTTP layer's error included.
                    log::warn!(
                        "Lighter get_trades failed (market_id={:?}, account_index={}, from={:?}, cursor={:?}): {}",
                        query.market_id,
                        credential.account_index(),
                        query.from_timestamp,
                        cursor,
                        scrub_auth(&format!("{e:#}")),
                    );
                    return Err(anyhow::Error::new(e).context("failed to fetch Lighter fills"));
                }
            };

            for trade in &response.trades {
                let Some(instrument_id) = self.registry.instrument_id(trade.market_id) else {
                    continue;
                };
                let Some(instrument) = self.core.cache().instrument(&instrument_id).cloned() else {
                    continue;
                };

                match parse_ws_fill_report(
                    trade,
                    credential.account_index(),
                    &instrument,
                    self.core.account_id,
                    ts_init,
                ) {
                    Ok(Some(report)) => {
                        self.dispatch.note_active_market(trade.market_id);

                        // Mass-status reconciliation must surface the original
                        // Nautilus cloid, not the venue's numeric echo.
                        let report = translate_fill_cloid(report, &self.dispatch.cloid_map);
                        if cmd.end.is_some_and(|end| report.ts_event > end) {
                            continue;
                        }

                        if !self.dispatch.mark_trade_seen(report.trade_id) {
                            log::debug!(
                                "Lighter duplicate trade {} ignored in HTTP fill reports",
                                report.trade_id,
                            );
                            continue;
                        }

                        reports.push(report);
                    }
                    Ok(None) => {}
                    Err(e) => log::warn!("Lighter fill parse failed: {e}"),
                }
            }

            match response.next_cursor {
                Some(next) if !next.is_empty() => cursor = Some(next),
                _ => break,
            }
        }

        log::debug!("Generated {} Lighter fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // No REST source; replay the WS-driven cache populated by the
        // consumption loop's `PositionSnapshot` arm.
        let reports = self.dispatch.snapshot_positions(cmd.instrument_id);
        log::debug!(
            "Lighter generate_position_status_reports: returning {} cached position reports",
            reports.len(),
        );
        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = self.clock.get_time_ns();

        // Push lookback_mins into the REST queries themselves so the venue
        // can scope the response. Without this, pagination has to walk full
        // trade history before local filtering, which can stall startup
        // reconciliation under the venue's 60 req/min REST quota.
        let lookback_start: Option<UnixNanos> = lookback_mins.map(|mins| {
            let cutoff_ns = ts_init
                .as_u64()
                .saturating_sub(mins.saturating_mul(60).saturating_mul(1_000_000_000));
            UnixNanos::from(cutoff_ns)
        });

        // open_only = false so the inactive-orders fan-out runs and surfaces
        // canceled / rejected / expired / filled orders that the engine
        // needs for reconciliation. The active markets set bounds the fan-out
        // to markets with known account activity.
        let order_cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            ts_init,
            false,
            None,
            lookback_start,
            None,
            None,
            None,
        );
        let fill_cmd = GenerateFillReports::new(
            UUID4::new(),
            ts_init,
            None,
            None,
            lookback_start,
            None,
            None,
            None,
        );
        let position_cmd =
            GeneratePositionStatusReports::new(UUID4::new(), ts_init, None, None, None, None, None);

        // Each sub-call degrades independently; see `unwrap_reports_or_warn`.
        let order_reports = unwrap_reports_or_warn(
            "order",
            self.generate_order_status_reports(&order_cmd).await,
        );
        let fill_reports =
            unwrap_reports_or_warn("fill", self.generate_fill_reports(fill_cmd).await);
        let position_reports = unwrap_reports_or_warn(
            "position",
            self.generate_position_status_reports(&position_cmd).await,
        );

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *LIGHTER_VENUE,
            ts_init,
            None,
        );
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        log::debug!(
            "Generated Lighter mass status: {} orders, {} fills, {} positions",
            mass_status.order_reports().len(),
            mass_status.fill_reports().len(),
            mass_status.position_reports().len(),
        );

        Ok(Some(mass_status))
    }
}

fn local_submit_denial_reason(
    order: &OrderAny,
    instrument: Option<&InstrumentAny>,
) -> Option<String> {
    if !is_lighter_supported_order_type(order.order_type()) {
        return Some(format!(
            "Unsupported order type for Lighter: {:?}",
            order.order_type()
        ));
    }

    if is_lighter_limit_style_order(order.order_type()) && order.price().is_none() {
        return Some("Lighter limit-style orders require a limit price".to_string());
    }

    if order.is_quote_quantity() {
        return Some(
            "Lighter orders do not support quote_quantity; submit base quantity instead"
                .to_string(),
        );
    }

    if order.display_qty().is_some() {
        return Some("Lighter orders do not support display_qty iceberg instructions".to_string());
    }

    if is_lighter_spot_order(order, instrument) && is_lighter_conditional_order(order.order_type())
    {
        return Some(format!(
            "Lighter spot markets do not support conditional order type {:?}",
            order.order_type()
        ));
    }

    nautilus_to_lighter_tif(
        order.order_type(),
        order.time_in_force(),
        order.is_post_only(),
    )
    .err()
    .map(|e| e.to_string())
}

fn is_grouped_order(order: &OrderAny) -> bool {
    matches!(
        order.contingency_type(),
        Some(contingency) if contingency != ContingencyType::NoContingency
    )
}

fn is_lighter_spot_order(order: &OrderAny, instrument: Option<&InstrumentAny>) -> bool {
    instrument.is_some_and(|instrument| matches!(instrument, InstrumentAny::CurrencyPair(_)))
        || product_type_from_instrument_id(&order.instrument_id()) == Some(LighterProductType::Spot)
}

fn is_lighter_supported_order_type(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::Market
            | OrderType::Limit
            | OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    )
}

fn is_lighter_limit_style_order(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
    )
}

fn is_lighter_conditional_order(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    )
}

async fn seed_active_markets_from_inactive_orders(
    http_client: &LighterHttpClient,
    dispatch: &WsDispatchState,
    credential: &Credential,
    auth: &str,
    between_timestamps: Option<String>,
) {
    let mut cursor: Option<String> = None;
    let mut orders_seen = 0_usize;

    loop {
        let response = match http_client
            .get_account_inactive_orders(&LighterAccountInactiveOrdersQuery {
                authorization: None,
                auth: Some(auth.to_string()),
                account_index: credential.account_index(),
                market_id: None,
                ask_filter: None,
                between_timestamps: between_timestamps.clone(),
                cursor: cursor.clone(),
                limit: LIGHTER_REST_PAGE_SIZE,
            })
            .await
        {
            Ok(response) => response,
            Err(e) => {
                log::warn!(
                    "Lighter active markets seed failed from inactive orders: {}",
                    scrub_auth(&format!("{e:#}")),
                );
                break;
            }
        };

        for order in &response.orders {
            dispatch.note_active_market(order.market_index);
            orders_seen += 1;
        }

        match response.next_cursor {
            Some(next) if !next.is_empty() => cursor = Some(next),
            _ => break,
        }
    }

    if orders_seen > 0 {
        log::debug!("Seeded Lighter active markets from {orders_seen} inactive order report(s)");
    }
}

fn cancel_order_from_cancel_all(
    cmd: &CancelAllOrders,
    client_order_id: ClientOrderId,
) -> CancelOrder {
    CancelOrder {
        trader_id: cmd.trader_id,
        client_id: cmd.client_id,
        strategy_id: cmd.strategy_id,
        instrument_id: cmd.instrument_id,
        client_order_id,
        venue_order_id: None,
        command_id: cmd.command_id,
        ts_init: cmd.ts_init,
        params: cmd.params.clone(),
        correlation_id: cmd.correlation_id,
        causation_id: cmd.causation_id,
    }
}

fn validate_order_amount(
    instrument: &InstrumentAny,
    quantity: Quantity,
    price_ticks: u32,
    price_precision: u8,
) -> anyhow::Result<()> {
    if let Some(min_quantity) = instrument.min_quantity() {
        anyhow::ensure!(
            quantity >= min_quantity,
            "quantity `{quantity}` below Lighter min_base_amount `{min_quantity}` for {}",
            instrument.id(),
        );
    }

    if let Some(min_notional) = instrument.min_notional() {
        let price = decimal_from_ticks(price_ticks, price_precision);
        let notional = quantity.as_decimal() * price;
        anyhow::ensure!(
            notional >= min_notional.as_decimal(),
            "order notional `{notional}` below Lighter min_quote_amount `{}` for {}",
            min_notional.as_decimal(),
            instrument.id(),
        );
    }

    Ok(())
}

fn decimal_from_ticks(ticks: u32, decimals: u8) -> Decimal {
    Decimal::from(ticks) / Decimal::from(10_i64.pow(u32::from(decimals)))
}

/// Route a venue `account_orders` payload through the tracked-event path
/// when the cloid is known, otherwise fall back to the existing
/// [`OrderStatusReport`] flow used for externally-managed orders.
fn dispatch_lighter_order(
    order: &crate::http::models::LighterOrder,
    dispatch: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    registry: &Arc<MarketRegistry>,
    account_id: AccountId,
    trader_id: TraderId,
    ts_init: UnixNanos,
) {
    let instrument_id = match registry.instrument_id(order.market_index) {
        Some(id) => id,
        None => {
            log::debug!(
                "Lighter order frame dropped: no instrument for market_index={}",
                order.market_index,
            );
            return;
        }
    };

    if let Some(idx) = registry.market_index(&instrument_id) {
        dispatch.note_active_market(idx);
    }

    let instrument = match LIGHTER_INSTRUMENT_CACHE.get(&instrument_id) {
        Some(inst) => inst.value().clone(),
        None => {
            log::debug!("Lighter order frame dropped: instrument {instrument_id} not in cache",);
            return;
        }
    };

    let resolved_cloid = resolve_cloid(order.client_order_id.as_str(), &dispatch.cloid_map);
    let venue_order_id = VenueOrderId::new(order.order_id.as_str());

    let identity = resolved_cloid.and_then(|cid| {
        dispatch
            .order_identities
            .get(&cid)
            .map(|entry| (cid, entry.value().clone()))
    });

    if let Some((cloid, identity)) = identity {
        dispatch.venue_id_map.insert(cloid, venue_order_id);

        // Pre-compute the parser's Open-frame context: accepted gate,
        // triggered gate, and shape diff against the stored snapshot.
        // The dispatcher owns the dispatch-state mutation and the parser
        // stays pure.
        let is_open_status =
            matches!(order.status, crate::common::enums::LighterOrderStatus::Open,);
        let current_shape = match lighter_order_shape(order, &instrument) {
            Ok(shape) => shape,
            Err(e) => {
                log::error!(
                    "Failed to compute Lighter order shape: error={e}, voi={venue_order_id}, cloid={cloid}",
                );
                return;
            }
        };
        let prior_shape = dispatch.snapshot_for(&cloid);
        let shape_changed = prior_shape
            .as_ref()
            .is_some_and(|prev| prev != &current_shape);
        let open_ctx = OpenFrameContext {
            accepted_already_emitted: dispatch.accepted_was_emitted(&cloid),
            triggered_already_emitted: dispatch.triggered_was_emitted(&cloid),
            shape_changed,
        };

        match parse_lighter_order_event(
            order,
            &instrument,
            &identity,
            cloid,
            account_id,
            trader_id,
            open_ctx,
            ts_init,
        ) {
            Ok(event_opt) => {
                // Refresh the stored snapshot for any tracked Open frame
                // so a synthesised `OrderAccepted` (fill-before-open or
                // fresh-trigger path) leaves a baseline behind for the
                // next diff. Without this seed `shape_changed` would
                // stay permanently false and a real later modify would
                // be missed. Filled / Canceled / Expired / Rejected
                // frames skip the refresh; identity cleanup in
                // `dispatch_tracked_order_event` removes the snapshot on
                // terminal events.
                if is_open_status {
                    dispatch.store_snapshot(cloid, current_shape);
                }

                if let Some(event) = event_opt {
                    dispatch_tracked_order_event(
                        event,
                        cloid,
                        venue_order_id,
                        &identity,
                        account_id,
                        trader_id,
                        emitter,
                        dispatch,
                        ts_init,
                    );
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to parse Lighter order event: error={e}, voi={venue_order_id}, cloid={cloid}",
                );
            }
        }
    } else {
        match parse_ws_order_status_report(order, &instrument, account_id, ts_init) {
            Ok(mut report) => {
                report = translate_order_cloid(report, &dispatch.cloid_map);

                if let Some(cloid) = &report.client_order_id {
                    dispatch.venue_id_map.insert(*cloid, report.venue_order_id);
                }

                if report.order_status.is_closed() {
                    evict_terminal_mappings(&report, &dispatch.venue_id_map);
                }

                log::debug!(
                    "Lighter OrderStatusReport: voi={} status={:?} cloid={:?}",
                    report.venue_order_id,
                    report.order_status,
                    report.client_order_id,
                );
                emitter.send_order_status_report(report);
            }
            Err(e) => {
                log::error!(
                    "Failed to parse Lighter order status report: error={e}, order_id={}",
                    order.order_id,
                );
            }
        }
    }
}

/// Route a venue `account_trades` payload through the tracked-event path
/// when the cloid is known, otherwise fall back to the existing
/// [`FillReport`] flow. Drops duplicate fill frames keyed by `trade_id`.
#[expect(
    clippy::too_many_arguments,
    reason = "consumption-loop dispatch threads identity and emitter context"
)]
fn dispatch_lighter_trade(
    trade: &crate::http::models::LighterTrade,
    dispatch: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    registry: &Arc<MarketRegistry>,
    account_id: AccountId,
    trader_id: TraderId,
    account_index: Option<i64>,
    ts_init: UnixNanos,
) {
    let Some(account_index) = account_index else {
        log::debug!("Lighter trade frame dropped: no credential / account_index available",);
        return;
    };

    let instrument_id = match registry.instrument_id(trade.market_id) {
        Some(id) => id,
        None => {
            log::debug!(
                "Lighter trade frame dropped: no instrument for market_id={}",
                trade.market_id,
            );
            return;
        }
    };

    if let Some(idx) = registry.market_index(&instrument_id) {
        dispatch.note_active_market(idx);
    }

    let instrument = match LIGHTER_INSTRUMENT_CACHE.get(&instrument_id) {
        Some(inst) => inst.value().clone(),
        None => {
            log::debug!("Lighter trade frame dropped: instrument {instrument_id} not in cache",);
            return;
        }
    };

    let user_is_bidder = trade.bid_account_id == account_index;
    let user_is_asker = trade.ask_account_id == account_index;
    if !user_is_bidder && !user_is_asker {
        // Defensive: the handler already filters foreign trades, so this
        // branch is rare in practice. Drop silently.
        return;
    }

    // Dedupe before dispatch so a duplicate frame on reconnect does not
    // double-book on either the tracked or untracked path.
    let trade_id = match parse_lighter_trade_id(trade) {
        Ok(id) => id,
        Err(e) => {
            log::error!("Lighter trade has invalid trade_id: {e}");
            return;
        }
    };

    if !dispatch.mark_trade_seen(trade_id) {
        log::debug!("Lighter duplicate trade {trade_id} ignored (already routed)",);
        return;
    }

    let raw_client_id = if user_is_bidder {
        trade
            .bid_client_id_str
            .as_deref()
            .map_or_else(|| trade.bid_client_id.to_string(), str::to_string)
    } else {
        trade
            .ask_client_id_str
            .as_deref()
            .map_or_else(|| trade.ask_client_id.to_string(), str::to_string)
    };
    let resolved_cloid = resolve_cloid(raw_client_id.as_str(), &dispatch.cloid_map);

    let identity = resolved_cloid.and_then(|cid| {
        dispatch
            .order_identities
            .get(&cid)
            .map(|entry| (cid, entry.value().clone()))
    });

    if let Some((cloid, identity)) = identity {
        // Synthesise an `OrderAccepted` first if one has not been
        // emitted yet: fills can race ahead of the matching `Open`
        // order frame.
        let venue_order_id = if user_is_bidder {
            trade.bid_id_str.as_deref().map_or_else(
                || VenueOrderId::new(trade.bid_id.to_string()),
                VenueOrderId::new,
            )
        } else {
            trade.ask_id_str.as_deref().map_or_else(
                || VenueOrderId::new(trade.ask_id.to_string()),
                VenueOrderId::new,
            )
        };
        ensure_accepted_emitted(
            cloid,
            venue_order_id,
            &identity,
            account_id,
            trader_id,
            emitter,
            dispatch,
            ts_init,
        );

        match parse_lighter_order_filled(
            trade,
            &instrument,
            &identity,
            cloid,
            account_id,
            trader_id,
            account_index,
            ts_init,
        ) {
            Ok(Some(filled)) => {
                log::debug!(
                    "Lighter OrderFilled: voi={} qty={} px={} liq={:?} cloid={cloid}",
                    filled.venue_order_id,
                    filled.last_qty,
                    filled.last_px,
                    filled.liquidity_side,
                );
                emitter.send_order_event(OrderEventAny::Filled(filled));
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("Failed to parse Lighter typed fill: error={e}, trade_id={trade_id}",);
            }
        }
    } else {
        match parse_ws_fill_report(trade, account_index, &instrument, account_id, ts_init) {
            Ok(Some(mut report)) => {
                report = translate_fill_cloid(report, &dispatch.cloid_map);
                log::debug!(
                    "Lighter FillReport: voi={} qty={} px={} liq={:?} cloid={:?}",
                    report.venue_order_id,
                    report.last_qty,
                    report.last_px,
                    report.liquidity_side,
                    report.client_order_id,
                );
                emitter.send_fill_report(report);
            }
            Ok(None) => {}
            Err(e) => {
                log::error!("Failed to parse Lighter fill report: error={e}, trade_id={trade_id}",);
            }
        }
    }
}

/// Send a [`ParsedOrderEvent`] to the engine and update dispatch state for
/// the originating cloid. Cleans up [`WsDispatchState::order_identities`]
/// on terminal events so subsequent stale frames take the untracked path.
#[expect(
    clippy::too_many_arguments,
    reason = "shared cleanup point across the typed-event variants"
)]
#[expect(
    clippy::needless_pass_by_value,
    reason = "event is destructured into typed OrderEventAny variants that consume the payload"
)]
fn dispatch_tracked_order_event(
    event: ParsedOrderEvent,
    cloid: ClientOrderId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    trader_id: TraderId,
    emitter: &ExecutionEventEmitter,
    dispatch: &WsDispatchState,
    ts_init: UnixNanos,
) {
    let is_terminal;

    match event {
        ParsedOrderEvent::Accepted(e) => {
            if dispatch.accepted_was_emitted(&cloid) {
                log::debug!("Skipping duplicate OrderAccepted for {cloid}");
                return;
            }
            dispatch.mark_accepted_emitted(cloid);
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Accepted(e));
        }
        ParsedOrderEvent::Triggered(e) => {
            if !dispatch.mark_triggered_emitted(cloid) {
                log::debug!("Skipping duplicate OrderTriggered for {cloid}");
                return;
            }
            ensure_accepted_emitted(
                cloid,
                venue_order_id,
                identity,
                account_id,
                trader_id,
                emitter,
                dispatch,
                ts_init,
            );
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Triggered(e));
        }
        ParsedOrderEvent::Updated(e) => {
            // Modify-as-restate: the venue echoes the post-modify order as
            // `Open`; `accepted_was_emitted` already gated parsing to
            // produce `Updated` instead of duplicate `Accepted`. No need
            // to re-synthesise the accept here.
            is_terminal = false;
            emitter.send_order_event(OrderEventAny::Updated(e));
        }
        ParsedOrderEvent::Canceled(e) => {
            ensure_accepted_emitted(
                cloid,
                venue_order_id,
                identity,
                account_id,
                trader_id,
                emitter,
                dispatch,
                ts_init,
            );
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Canceled(e));
        }
        ParsedOrderEvent::Expired(e) => {
            ensure_accepted_emitted(
                cloid,
                venue_order_id,
                identity,
                account_id,
                trader_id,
                emitter,
                dispatch,
                ts_init,
            );
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Expired(e));
        }
        ParsedOrderEvent::Rejected(e) => {
            is_terminal = true;
            emitter.send_order_event(OrderEventAny::Rejected(e));
        }
    }

    if is_terminal {
        dispatch.venue_id_map.remove(&cloid);
        dispatch.forget_order_identity(&cloid);
    }
}

/// Synthesise an `OrderAccepted` event if one has not yet been emitted for
/// `cloid`. Mirrors the BitMEX dispatch helper of the same name.
#[expect(
    clippy::too_many_arguments,
    reason = "synthesised events need the full identity context to populate the event"
)]
fn ensure_accepted_emitted(
    cloid: ClientOrderId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    account_id: AccountId,
    trader_id: TraderId,
    emitter: &ExecutionEventEmitter,
    dispatch: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if dispatch.accepted_was_emitted(&cloid) {
        return;
    }
    dispatch.mark_accepted_emitted(cloid);
    let accepted = OrderAccepted::new(
        trader_id,
        identity.strategy_id,
        identity.instrument_id,
        cloid,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_init,
        ts_init,
        false,
    );
    emitter.send_order_event(OrderEventAny::Accepted(accepted));
}

#[cfg(test)]
mod tests {
    use std::{
        cell::RefCell,
        rc::Rc,
        sync::{Arc, atomic::AtomicUsize},
    };

    use axum::{
        Router,
        routing::{get, post},
    };
    use nautilus_common::{
        cache::Cache,
        clock::TestClock,
        factories::OrderFactory,
        messages::{ExecutionEvent, ExecutionReport as EngineExecutionReport},
        testing::wait_until_async,
    };
    use nautilus_model::{
        data::QuoteTick,
        enums::{OrderStatus, TimeInForce},
        events::{OrderEventAny, OrderPendingCancel},
        identifiers::{
            InstrumentId, OrderListId, StrategyId, Symbol, TradeId, TraderId, VenueOrderId,
        },
        instruments::CryptoPerpetual,
        orders::{LimitOrder, OrderList},
        types::{Currency, Money},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::{LighterEnvironment, LighterProductType},
        http::models::{LighterNextNonce, LighterSendTxBatchResponse},
        signing::tx::TX_HASH_BYTES,
    };

    const TEST_PRIVATE_KEY: &str =
        "0b8e0f63c24d8baacd9d29ad4e9a4b73c4a8d2bb8b16dc4fa9d7c2e1d3a8b1f0e8d3a4c5b6e7f001";
    const TEST_ACCOUNT_INDEX: u64 = 12345;
    const TEST_ACCOUNT_INDEX_I64: i64 = 12345;
    const TEST_API_KEY_INDEX: u8 = 5;
    const TEST_NEXT_NONCE: i64 = 42;
    const TEST_MARKET_INDEX: i16 = 0;

    fn trader_id() -> TraderId {
        TraderId::from("TRADER-001")
    }

    fn client_id() -> ClientId {
        ClientId::from("LIGHTER")
    }

    fn account_id() -> AccountId {
        AccountId::from("LIGHTER-001")
    }

    fn strategy_id() -> StrategyId {
        StrategyId::from("S-001")
    }

    fn test_credential() -> Credential {
        Credential::new(TEST_API_KEY_INDEX, TEST_PRIVATE_KEY, TEST_ACCOUNT_INDEX).unwrap()
    }

    fn test_config() -> LighterExecClientConfig {
        LighterExecClientConfig {
            trader_id: trader_id(),
            account_id: account_id(),
            account_index: Some(TEST_ACCOUNT_INDEX),
            api_key_index: Some(TEST_API_KEY_INDEX),
            private_key: Some(TEST_PRIVATE_KEY.to_string()),
            base_url_http: Some("http://127.0.0.1:1".to_string()),
            base_url_ws: Some("ws://127.0.0.1:1/stream".to_string()),
            proxy_url: None,
            environment: LighterEnvironment::Testnet,
            http_timeout_secs: 1,
            ws_timeout_secs: 1,
            active_markets: Vec::new(),
            market_order_slippage_bps: 50,
            rest_quota_per_min: None,
            sendtx_quota_per_min: None,
            transport_backend: Default::default(),
        }
    }

    fn create_execution_client() -> (
        LighterExecutionClient,
        Rc<RefCell<Cache>>,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        create_execution_client_with_config(test_config())
    }

    fn create_execution_client_with_config(
        config: LighterExecClientConfig,
    ) -> (
        LighterExecutionClient,
        Rc<RefCell<Cache>>,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            trader_id(),
            client_id(),
            *LIGHTER_VENUE,
            OmsType::Netting,
            account_id(),
            AccountType::Margin,
            None,
            cache.clone(),
        );

        let mut client = LighterExecutionClient::new(core, config).unwrap();
        client.dispatch.nonce_manager.refresh(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX,
            TEST_NEXT_NONCE,
        );

        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        client.emitter.set_sender(sender);

        (client, cache, receiver)
    }

    fn register_test_instrument(
        client: &LighterExecutionClient,
        cache: &Rc<RefCell<Cache>>,
    ) -> InstrumentId {
        let instrument_id =
            client
                .registry
                .insert(TEST_MARKET_INDEX, "ETH", LighterProductType::Perp);
        let instrument = InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("ETH-PERP"),
            Currency::from("ETH"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            Some(Money::from("10.000000 USDC")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ));

        cache.borrow_mut().add_instrument(instrument).unwrap();

        instrument_id
    }

    fn test_order_factory() -> OrderFactory {
        let clock = Rc::new(RefCell::new(TestClock::new()));
        OrderFactory::new(
            trader_id(),
            strategy_id(),
            Some(0),
            Some(0),
            clock,
            false,
            false,
        )
    }

    fn test_limit_order(
        factory: &mut OrderFactory,
        instrument_id: InstrumentId,
        client_order_id: &str,
    ) -> OrderAny {
        test_limit_order_with(
            factory,
            instrument_id,
            client_order_id,
            OrderSide::Buy,
            TimeInForce::Gtc,
            None,
            false,
        )
    }

    fn test_limit_order_with(
        factory: &mut OrderFactory,
        instrument_id: InstrumentId,
        client_order_id: &str,
        side: OrderSide,
        tif: TimeInForce,
        expire_time: Option<UnixNanos>,
        reduce_only: bool,
    ) -> OrderAny {
        factory.limit(
            instrument_id,
            side,
            Quantity::from("0.1000"),
            Price::from("2361.31"),
            Some(tif),
            expire_time,
            Some(false),
            Some(reduce_only),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(ClientOrderId::from(client_order_id)),
        )
    }

    fn cache_order(cache: &Rc<RefCell<Cache>>, order: OrderAny) {
        cache
            .borrow_mut()
            .add_order(order, None, Some(client_id()), false)
            .unwrap();
    }

    fn cache_accepted_order(
        cache: &Rc<RefCell<Cache>>,
        order: OrderAny,
        venue_order_id: VenueOrderId,
    ) -> (InstrumentId, ClientOrderId) {
        let instrument_id = order.instrument_id();
        let client_order_id = order.client_order_id();
        cache_order(cache, order);

        let accepted = OrderEventAny::Accepted(OrderAccepted::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            client_order_id,
            venue_order_id,
            account_id(),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
        ));
        cache.borrow_mut().update_order(&accepted).unwrap();

        (instrument_id, client_order_id)
    }

    fn cache_pending_cancel_order(
        cache: &Rc<RefCell<Cache>>,
        order: OrderAny,
        venue_order_id: VenueOrderId,
    ) {
        let (instrument_id, client_order_id) = cache_accepted_order(cache, order, venue_order_id);

        let pending_cancel = OrderEventAny::PendingCancel(OrderPendingCancel::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            client_order_id,
            account_id(),
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            Some(venue_order_id),
        ));
        cache.borrow_mut().update_order(&pending_cancel).unwrap();
    }

    fn submit_order_list_command(orders: &[OrderAny], order_list_id: &str) -> SubmitOrderList {
        let order_list = OrderList::new(
            OrderListId::from(order_list_id),
            orders[0].instrument_id(),
            strategy_id(),
            orders.iter().map(|order| order.client_order_id()).collect(),
            UnixNanos::default(),
        );
        let order_inits = orders
            .iter()
            .map(|order| order.init_event().clone())
            .collect();

        SubmitOrderList::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            order_list,
            order_inits,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
        )
    }

    fn test_contingent_limit_order(
        instrument_id: InstrumentId,
        client_order_id: &str,
        order_list_id: &str,
    ) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            trader_id(),
            strategy_id(),
            instrument_id,
            ClientOrderId::from(client_order_id),
            OrderSide::Buy,
            Quantity::from("0.1000"),
            Price::from("2361.31"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            false,
            None,
            None,
            None,
            Some(ContingencyType::Oco),
            Some(OrderListId::from(order_list_id)),
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))
    }

    async fn recv_order_event(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> OrderEventAny {
        let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
            .await
            .expect("timed out waiting for execution event")
            .expect("execution event channel closed");

        match event {
            ExecutionEvent::Order(event) => event,
            event => panic!("expected order event, was {event:?}"),
        }
    }

    fn assert_nonce_reusable(dispatch: &WsDispatchState) {
        assert_eq!(
            dispatch
                .nonce_manager
                .last_issued(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(TEST_NEXT_NONCE - 1),
        );
        assert_eq!(
            dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            TEST_NEXT_NONCE,
        );
    }

    #[tokio::test]
    async fn auth_token_rotation_retries_failed_mint() {
        let credential = test_credential();
        let channels = auth_token_rotation_channels(TEST_ACCOUNT_INDEX_I64);
        let cancellation_token = CancellationToken::new();
        let mint_attempts = Arc::new(AtomicUsize::new(0));
        let subscribe_attempts = Arc::new(AtomicUsize::new(0));

        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            refresh_auth_token_until_rotated(
                &credential,
                &channels,
                &cancellation_token,
                AuthTokenRefreshBackoff {
                    initial_delay: Duration::from_millis(10),
                    max_delay: Duration::from_millis(20),
                    window: Duration::from_secs(1),
                },
                {
                    let mint_attempts = Arc::clone(&mint_attempts);
                    move |_| {
                        let attempt = mint_attempts.fetch_add(1, Ordering::AcqRel);
                        if attempt == 0 {
                            Err(anyhow::anyhow!("mint unavailable"))
                        } else {
                            Ok(format!("token-{attempt}"))
                        }
                    }
                },
                {
                    let subscribe_attempts = Arc::clone(&subscribe_attempts);
                    move |_channel, _token| {
                        let subscribe_attempts = Arc::clone(&subscribe_attempts);
                        async move {
                            subscribe_attempts.fetch_add(1, Ordering::AcqRel);
                            Ok::<(), crate::websocket::error::LighterWsError>(())
                        }
                    }
                },
            ),
        )
        .await
        .expect("rotation retry must complete within the test window");

        assert_eq!(outcome, AuthTokenRefreshOutcome::Rotated);
        assert_eq!(
            mint_attempts.load(Ordering::Acquire),
            2,
            "failed mint must retry before the next refresh interval",
        );
        assert_eq!(
            subscribe_attempts.load(Ordering::Acquire),
            channels.len(),
            "subscriptions must wait until token mint succeeds",
        );
    }

    #[tokio::test]
    async fn auth_token_rotation_retries_failed_resubscribe() {
        let credential = test_credential();
        let channels = auth_token_rotation_channels(TEST_ACCOUNT_INDEX_I64);
        let cancellation_token = CancellationToken::new();
        let mint_attempts = Arc::new(AtomicUsize::new(0));
        let subscribe_attempts = Arc::new(AtomicUsize::new(0));

        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            refresh_auth_token_until_rotated(
                &credential,
                &channels,
                &cancellation_token,
                AuthTokenRefreshBackoff {
                    initial_delay: Duration::from_millis(10),
                    max_delay: Duration::from_millis(20),
                    window: Duration::from_secs(1),
                },
                {
                    let mint_attempts = Arc::clone(&mint_attempts);
                    move |_| {
                        let attempt = mint_attempts.fetch_add(1, Ordering::AcqRel);
                        Ok(format!("token-{attempt}"))
                    }
                },
                {
                    let subscribe_attempts = Arc::clone(&subscribe_attempts);
                    move |_channel, _token| {
                        let subscribe_attempts = Arc::clone(&subscribe_attempts);
                        async move {
                            let attempt = subscribe_attempts.fetch_add(1, Ordering::AcqRel);
                            if attempt == 0 {
                                Err(crate::websocket::error::LighterWsError::Client(
                                    "handler unavailable".to_string(),
                                ))
                            } else {
                                Ok(())
                            }
                        }
                    }
                },
            ),
        )
        .await
        .expect("rotation retry must complete within the test window");

        assert_eq!(outcome, AuthTokenRefreshOutcome::Rotated);
        assert_eq!(
            mint_attempts.load(Ordering::Acquire),
            2,
            "failed resubscribe must trigger a fresh auth-token mint",
        );
        assert_eq!(
            subscribe_attempts.load(Ordering::Acquire),
            channels.len() * 2,
            "failed resubscribe must retry the private account-channel set",
        );
    }

    #[tokio::test]
    async fn auth_token_rotation_exhausts_retry_window() {
        let credential = test_credential();
        let channels = auth_token_rotation_channels(TEST_ACCOUNT_INDEX_I64);
        let cancellation_token = CancellationToken::new();
        let mint_attempts = Arc::new(AtomicUsize::new(0));

        let refresh = refresh_auth_token_until_rotated(
            &credential,
            &channels,
            &cancellation_token,
            AuthTokenRefreshBackoff {
                initial_delay: Duration::from_millis(100),
                max_delay: Duration::from_millis(100),
                window: Duration::from_secs(1),
            },
            {
                let mint_attempts = Arc::clone(&mint_attempts);
                move |_| {
                    mint_attempts.fetch_add(1, Ordering::AcqRel);
                    Err(anyhow::anyhow!("mint unavailable"))
                }
            },
            |_channel, _token| async { Ok::<(), crate::websocket::error::LighterWsError>(()) },
        );
        let observe_retry = wait_until_async(
            || async { mint_attempts.load(Ordering::Acquire) > 1 },
            Duration::from_secs(2),
        );

        let (outcome, ()) = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::join!(refresh, observe_retry)
        })
        .await
        .expect("rotation retry exhaustion must complete within the test window");

        assert_eq!(outcome, AuthTokenRefreshOutcome::Exhausted);
        assert!(
            mint_attempts.load(Ordering::Acquire) > 1,
            "persistent mint failure must retry until the window is exhausted",
        );
    }

    #[tokio::test]
    async fn auth_token_rotation_cancels_during_retry_backoff() {
        let credential = test_credential();
        let channels = auth_token_rotation_channels(TEST_ACCOUNT_INDEX_I64);
        let cancellation_token = CancellationToken::new();
        let mint_attempts = Arc::new(AtomicUsize::new(0));

        let cancel = cancellation_token.clone();
        let cancel_after_first_attempt = wait_until_async(
            || async { mint_attempts.load(Ordering::Acquire) > 0 },
            Duration::from_secs(2),
        );

        let refresh = refresh_auth_token_until_rotated(
            &credential,
            &channels,
            &cancellation_token,
            AuthTokenRefreshBackoff {
                initial_delay: Duration::from_secs(2),
                max_delay: Duration::from_secs(2),
                window: Duration::from_secs(5),
            },
            {
                let mint_attempts = Arc::clone(&mint_attempts);
                move |_| {
                    mint_attempts.fetch_add(1, Ordering::AcqRel);
                    Err(anyhow::anyhow!("mint unavailable"))
                }
            },
            |_channel, _token| async { Ok::<(), crate::websocket::error::LighterWsError>(()) },
        );

        let cancel_task = async move {
            cancel_after_first_attempt.await;
            cancel.cancel();
        };

        let (outcome, ()) = tokio::time::timeout(Duration::from_secs(6), async {
            tokio::join!(refresh, cancel_task)
        })
        .await
        .expect("rotation cancellation must complete within the test window");

        assert_eq!(outcome, AuthTokenRefreshOutcome::Cancelled);
        assert_eq!(
            mint_attempts.load(Ordering::Acquire),
            1,
            "cancellation during backoff must stop before the next retry",
        );
    }

    #[rstest]
    #[case::rotated(AuthTokenRefreshOutcome::Rotated, Some(AUTH_TOKEN_REFRESH_INTERVAL))]
    #[case::cancelled(AuthTokenRefreshOutcome::Cancelled, None)]
    #[case::exhausted(
        AuthTokenRefreshOutcome::Exhausted,
        Some(AUTH_TOKEN_REFRESH_RETRY_MAX_DELAY)
    )]
    fn auth_token_refresh_next_delay_matches_outcome(
        #[case] outcome: AuthTokenRefreshOutcome,
        #[case] expected: Option<Duration>,
    ) {
        assert_eq!(auth_token_refresh_next_delay(outcome), expected);
    }

    #[rstest]
    fn auth_token_rotation_channels_match_private_account_streams() {
        assert_eq!(
            auth_token_rotation_channels(TEST_ACCOUNT_INDEX_I64),
            [
                LighterWsChannel::AccountAllOrders(TEST_ACCOUNT_INDEX_I64),
                LighterWsChannel::AccountAllTrades(TEST_ACCOUNT_INDEX_I64),
                LighterWsChannel::AccountAllPositions(TEST_ACCOUNT_INDEX_I64),
                LighterWsChannel::AccountAllAssets(TEST_ACCOUNT_INDEX_I64),
                LighterWsChannel::UserStats(TEST_ACCOUNT_INDEX_I64),
            ],
        );
    }

    #[rstest]
    #[case::below_max(
        Duration::from_millis(10),
        Duration::from_millis(100),
        Duration::from_millis(20)
    )]
    #[case::at_max(
        Duration::from_millis(100),
        Duration::from_millis(100),
        Duration::from_millis(100)
    )]
    #[case::overflow_clamps(Duration::MAX, Duration::from_secs(300), Duration::from_secs(300))]
    fn next_auth_token_refresh_retry_delay_doubles_and_caps(
        #[case] current: Duration,
        #[case] max: Duration,
        #[case] expected: Duration,
    ) {
        assert_eq!(next_auth_token_refresh_retry_delay(current, max), expected);
    }

    #[tokio::test]
    async fn tx_send_sequencer_blocks_higher_nonce_until_lower_batch_releases() {
        let sequencer = TxSendSequencer::new();
        let mut lower_a =
            sequencer.reserve(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX, TEST_NEXT_NONCE);
        let mut lower_b = sequencer.reserve(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX,
            TEST_NEXT_NONCE + 1,
        );
        let mut higher = sequencer.reserve(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX,
            TEST_NEXT_NONCE + 2,
        );
        let (sent_tx, mut sent_rx) = tokio::sync::mpsc::unbounded_channel();

        let higher_task = tokio::spawn(async move {
            higher.wait_for_turn().await;
            sent_tx.send(higher.nonce).unwrap();
            higher.release();
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(50), sent_rx.recv())
                .await
                .is_err(),
            "higher nonce must wait while the lower batch is pending",
        );

        {
            let lower_reservations = [&lower_a, &lower_b];
            tokio::time::timeout(
                Duration::from_millis(50),
                wait_for_tx_send_reservations(&lower_reservations),
            )
            .await
            .expect("lower batch must already have the send turn");
        }

        lower_a.release();
        lower_b.release();

        let sent = tokio::time::timeout(Duration::from_secs(2), sent_rx.recv())
            .await
            .expect("higher nonce must proceed after lower batch releases")
            .expect("send channel must stay open");
        higher_task.await.unwrap();

        assert_eq!(
            sent,
            TEST_NEXT_NONCE + 2,
            "higher nonce must send after the lower batch releases",
        );
    }

    #[tokio::test]
    async fn tx_send_reservation_drop_releases_lower_nonce() {
        let sequencer = TxSendSequencer::new();
        let lower = sequencer.reserve(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX, TEST_NEXT_NONCE);
        let mut higher = sequencer.reserve(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX,
            TEST_NEXT_NONCE + 1,
        );
        let (sent_tx, mut sent_rx) = tokio::sync::mpsc::unbounded_channel();

        let higher_task = tokio::spawn(async move {
            higher.wait_for_turn().await;
            sent_tx.send(higher.nonce).unwrap();
            higher.release();
        });

        assert!(
            tokio::time::timeout(Duration::from_millis(50), sent_rx.recv())
                .await
                .is_err(),
            "higher nonce must wait while the lower reservation is pending",
        );

        drop(lower);

        let sent = tokio::time::timeout(Duration::from_secs(2), sent_rx.recv())
            .await
            .expect("higher nonce must proceed after lower reservation drops")
            .expect("send channel must stay open");
        higher_task.await.unwrap();

        assert_eq!(
            sent,
            TEST_NEXT_NONCE + 1,
            "higher nonce must send after the lower reservation drops",
        );
    }

    #[tokio::test]
    async fn tx_send_sequencer_keeps_nonce_streams_independent() {
        let sequencer = TxSendSequencer::new();
        let _other_account = sequencer.reserve(
            TEST_ACCOUNT_INDEX_I64 + 1,
            TEST_API_KEY_INDEX,
            TEST_NEXT_NONCE,
        );
        let _other_api_key = sequencer.reserve(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX + 1,
            TEST_NEXT_NONCE,
        );
        let mut current = sequencer.reserve(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX,
            TEST_NEXT_NONCE + 10,
        );

        tokio::time::timeout(Duration::from_millis(50), current.wait_for_turn())
            .await
            .expect("lower nonces for other keys must not block this key");
        current.release();
    }

    #[rstest]
    fn tx_dispatch_guard_rolls_back_nonce_and_cloid_when_armed() {
        let dispatch = WsDispatchState::new();
        let credential = test_credential();
        dispatch
            .nonce_manager
            .refresh(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX, TEST_NEXT_NONCE);

        let cloid = ClientOrderId::from("O-GUARD-ARMED");
        let client_order_index = dispatch.derive_client_order_index(&cloid);
        dispatch.register_cloid(client_order_index, cloid);
        let nonce = dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();

        {
            let _guard = TxDispatchGuard::new(
                dispatch.clone(),
                &credential,
                Some(client_order_index),
                nonce,
            );
        }

        assert_nonce_reusable(&dispatch);
        assert!(dispatch.cloid_map.get(&client_order_index).is_none());
    }

    #[rstest]
    fn tx_dispatch_guard_rolls_back_nonce_without_cloid_when_armed() {
        let dispatch = WsDispatchState::new();
        let credential = test_credential();
        dispatch
            .nonce_manager
            .refresh(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX, TEST_NEXT_NONCE);
        let nonce = dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();

        {
            let _guard = TxDispatchGuard::new(dispatch.clone(), &credential, None, nonce);
        }

        assert_nonce_reusable(&dispatch);
        assert!(dispatch.cloid_map.is_empty());
    }

    #[rstest]
    fn tx_dispatch_guard_preserves_nonce_and_cloid_when_disarmed() {
        let dispatch = WsDispatchState::new();
        let credential = test_credential();
        dispatch
            .nonce_manager
            .refresh(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX, TEST_NEXT_NONCE);

        let cloid = ClientOrderId::from("O-GUARD-DISARMED");
        let client_order_index = dispatch.derive_client_order_index(&cloid);
        dispatch.register_cloid(client_order_index, cloid);
        let nonce = dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();

        {
            let mut guard = TxDispatchGuard::new(
                dispatch.clone(),
                &credential,
                Some(client_order_index),
                nonce,
            );
            guard.disarm();
        }

        assert_eq!(
            dispatch
                .nonce_manager
                .last_issued(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(TEST_NEXT_NONCE),
        );
        assert_eq!(
            dispatch
                .cloid_map
                .get(&client_order_index)
                .map(|entry| *entry.value()),
            Some(cloid),
        );
    }

    #[tokio::test]
    async fn submit_order_send_failure_emits_submitted_then_rejected_and_rolls_back() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-SUBMIT-FAIL");
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        client.submit_order(command).unwrap();

        let submitted = recv_order_event(&mut rx).await;
        let rejected = recv_order_event(&mut rx).await;

        match submitted {
            OrderEventAny::Submitted(event) => {
                assert_eq!(event.client_order_id, order.client_order_id());
                assert_eq!(event.instrument_id, instrument_id);
            }
            event => panic!("expected submitted event, was {event:?}"),
        }

        match rejected {
            OrderEventAny::Rejected(event) => {
                assert_eq!(event.client_order_id, order.client_order_id());
                assert_eq!(event.instrument_id, instrument_id);
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter submit_order dispatch failed"),
                );
                assert!(event.reason.as_str().contains("handler unavailable"));
            }
            event => panic!("expected rejected event, was {event:?}"),
        }

        assert!(client.dispatch.cloid_map.get(&client_order_index).is_none());
        assert_nonce_reusable(&client.dispatch);
        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            0,
            "local-send-failure must remove the pending entry by nonce",
        );
    }

    #[tokio::test]
    async fn submit_sell_order_send_failure_dispatches_and_rolls_back() {
        // Mirror of the buy-side test for OrderSide::Sell; covers the
        // `is_ask=true` branch of the CreateOrderTxInfo payload.
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order_with(
            &mut factory,
            instrument_id,
            "O-SUBMIT-FAIL-SELL",
            OrderSide::Sell,
            TimeInForce::Gtc,
            None,
            false,
        );
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        client.submit_order(command).unwrap();

        let _submitted = recv_order_event(&mut rx).await;
        let rejected = recv_order_event(&mut rx).await;

        match rejected {
            OrderEventAny::Rejected(event) => {
                assert_eq!(event.client_order_id, order.client_order_id());
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter submit_order dispatch failed"),
                );
            }
            event => panic!("expected rejected event, was {event:?}"),
        }

        assert!(client.dispatch.cloid_map.get(&client_order_index).is_none());
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_gtd_order_with_explicit_expiry_dispatches_and_rolls_back() {
        // Covers the GTD branch in `order_expiry_for`: an explicit
        // expire_time must propagate as venue millis through the dispatch
        // path. Asserts the order reaches the dispatch step (rejected here
        // by handler unavailability, not by the adapter validating GTD).
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let expiry = UnixNanos::from(1_900_000_000_000_000_000u64);
        let order = test_limit_order_with(
            &mut factory,
            instrument_id,
            "O-SUBMIT-FAIL-GTD",
            OrderSide::Buy,
            TimeInForce::Gtd,
            Some(expiry),
            false,
        );
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        client.submit_order(command).unwrap();

        let _submitted = recv_order_event(&mut rx).await;
        let rejected = recv_order_event(&mut rx).await;

        match rejected {
            OrderEventAny::Rejected(event) => {
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter submit_order dispatch failed"),
                );
            }
            event => panic!("expected rejected event, was {event:?}"),
        }

        assert!(client.dispatch.cloid_map.get(&client_order_index).is_none());
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_reduce_only_order_dispatches_and_rolls_back() {
        // The adapter does not reject `reduce_only=true` locally; the flag
        // flows into `OrderInfo.reduce_only` and the venue enforces the
        // "must be reducing an existing position" rule. This pins the
        // adapter pass-through and confirms the dispatch path tolerates
        // the flag.
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order_with(
            &mut factory,
            instrument_id,
            "O-SUBMIT-FAIL-REDUCE",
            OrderSide::Sell,
            TimeInForce::Gtc,
            None,
            true,
        );
        assert!(order.is_reduce_only());
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        client.submit_order(command).unwrap();

        let _submitted = recv_order_event(&mut rx).await;
        let rejected = recv_order_event(&mut rx).await;

        match rejected {
            OrderEventAny::Rejected(event) => {
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter submit_order dispatch failed"),
                );
            }
            event => panic!("expected rejected event, was {event:?}"),
        }

        assert!(client.dispatch.cloid_map.get(&client_order_index).is_none());
        assert_nonce_reusable(&client.dispatch);
    }

    async fn spawn_send_tx_batch_server() -> String {
        let body = serde_json::to_string(&LighterSendTxBatchResponse {
            code: 200,
            message: None,
            tx_hash: vec!["0xabc".to_string(), "0xdef".to_string()],
            predicted_execution_time_ms: 0,
            volume_quota_remaining: None,
        })
        .unwrap();
        let app = Router::new().route(
            "/api/v1/sendTxBatch",
            post(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn submit_order_list_success_advances_nonce_baseline() {
        let mut config = test_config();
        config.base_url_http = Some(spawn_send_tx_batch_server().await);
        let (client, cache, mut rx) = create_execution_client_with_config(config);
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order_a = test_limit_order(&mut factory, instrument_id, "O-LIST-OK-A");
        let order_b = test_limit_order(&mut factory, instrument_id, "O-LIST-OK-B");
        cache_order(&cache, order_a.clone());
        cache_order(&cache, order_b.clone());

        let command = submit_order_list_command(&[order_a, order_b], "OL-OK");
        client.submit_order_list(command).unwrap();

        let submitted_a = recv_order_event(&mut rx).await;
        let submitted_b = recv_order_event(&mut rx).await;
        assert!(matches!(submitted_a, OrderEventAny::Submitted(_)));
        assert!(matches!(submitted_b, OrderEventAny::Submitted(_)));
        wait_for_spawned_tasks(&client).await;

        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .baseline(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(TEST_NEXT_NONCE + 1),
            "batch success must advance baseline to max batched nonce",
        );
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            TEST_NEXT_NONCE + 2,
        );
    }

    async fn spawn_invalid_nonce_batch_server(venue_next_nonce: i64) -> String {
        let reject_body = serde_json::json!({
            "code": LIGHTER_ERROR_CODE_INVALID_NONCE,
            "message": "invalid nonce",
        })
        .to_string();
        let nonce_body = serde_json::to_string(&LighterNextNonce {
            code: 200,
            message: None,
            nonce: venue_next_nonce,
        })
        .unwrap();
        let app = Router::new()
            .route(
                "/api/v1/sendTxBatch",
                post(move || {
                    let body = reject_body.clone();
                    async move { body }
                }),
            )
            .route(
                "/api/v1/nextNonce",
                get(move || {
                    let body = nonce_body.clone();
                    async move { body }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn submit_order_list_invalid_nonce_rejection_hard_refreshes() {
        let venue_next_nonce = 200;
        let mut config = test_config();
        config.base_url_http = Some(spawn_invalid_nonce_batch_server(venue_next_nonce).await);
        let (client, cache, mut rx) = create_execution_client_with_config(config);
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order_a = test_limit_order(&mut factory, instrument_id, "O-LIST-WEDGE-A");
        let order_b = test_limit_order(&mut factory, instrument_id, "O-LIST-WEDGE-B");
        cache_order(&cache, order_a.clone());
        cache_order(&cache, order_b.clone());

        let command = submit_order_list_command(&[order_a, order_b], "OL-WEDGE");
        client.submit_order_list(command).unwrap();

        for _ in 0..2 {
            let event = recv_order_event(&mut rx).await;
            assert!(matches!(event, OrderEventAny::Submitted(_)));
        }

        for _ in 0..2 {
            let event = recv_order_event(&mut rx).await;
            assert!(matches!(event, OrderEventAny::Rejected(_)));
        }

        wait_for_spawned_tasks(&client).await;

        // Realignment must move allocation below the local rollback point
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            venue_next_nonce,
            "batch invalid-nonce rejection must hard-refresh allocation",
        );
    }

    #[tokio::test]
    async fn batch_cancel_orders_invalid_nonce_rejection_hard_refreshes() {
        let venue_next_nonce = 300;
        let mut config = test_config();
        config.base_url_http = Some(spawn_invalid_nonce_batch_server(venue_next_nonce).await);
        let (client, cache, _rx) = create_execution_client_with_config(config);
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let cancels = ["O-WEDGE-CXL-A", "O-WEDGE-CXL-B"]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let order = test_limit_order(&mut factory, instrument_id, id);
                let client_order_id = order.client_order_id();
                let venue_order_id = VenueOrderId::from(format!("{}", 321 + i).as_str());
                cache_pending_cancel_order(&cache, order, venue_order_id);

                CancelOrder::new(
                    trader_id(),
                    Some(client_id()),
                    strategy_id(),
                    instrument_id,
                    client_order_id,
                    Some(venue_order_id),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();

        let command = BatchCancelOrders::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            cancels,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.batch_cancel_orders(command).unwrap();
        wait_for_spawned_tasks(&client).await;

        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            venue_next_nonce,
            "batch invalid-nonce rejection must hard-refresh allocation",
        );
    }

    #[tokio::test]
    async fn batch_cancel_orders_success_advances_nonce_baseline() {
        let mut config = test_config();
        config.base_url_http = Some(spawn_send_tx_batch_server().await);
        let (client, cache, _rx) = create_execution_client_with_config(config);
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let cancels = ["O-BATCH-OK-A", "O-BATCH-OK-B"]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let order = test_limit_order(&mut factory, instrument_id, id);
                let client_order_id = order.client_order_id();
                cache_order(&cache, order);

                CancelOrder::new(
                    trader_id(),
                    Some(client_id()),
                    strategy_id(),
                    instrument_id,
                    client_order_id,
                    Some(VenueOrderId::from(format!("{}", 123 + i).as_str())),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();

        let command = BatchCancelOrders::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            cancels,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.batch_cancel_orders(command).unwrap();
        wait_for_spawned_tasks(&client).await;

        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .baseline(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(TEST_NEXT_NONCE + 1),
            "batch success must advance baseline to max batched nonce",
        );
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            TEST_NEXT_NONCE + 2,
        );
    }

    #[tokio::test]
    async fn submit_order_list_send_failure_emits_submitted_then_rejected_and_rolls_back() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order_a = test_limit_order(&mut factory, instrument_id, "O-LIST-FAIL-A");
        let order_b = test_limit_order(&mut factory, instrument_id, "O-LIST-FAIL-B");
        let index_a = client
            .dispatch
            .derive_client_order_index(&order_a.client_order_id());
        let index_b = client
            .dispatch
            .derive_client_order_index(&order_b.client_order_id());
        cache_order(&cache, order_a.clone());
        cache_order(&cache, order_b.clone());

        let command = submit_order_list_command(&[order_a.clone(), order_b.clone()], "OL-FAIL");
        client.submit_order_list(command).unwrap();

        let submitted_a = recv_order_event(&mut rx).await;
        let submitted_b = recv_order_event(&mut rx).await;
        let rejected_a = recv_order_event(&mut rx).await;
        let rejected_b = recv_order_event(&mut rx).await;

        for (event, expected) in [
            (submitted_a, order_a.client_order_id()),
            (submitted_b, order_b.client_order_id()),
        ] {
            match event {
                OrderEventAny::Submitted(e) => assert_eq!(e.client_order_id, expected),
                other => panic!("expected Submitted, was {other:?}"),
            }
        }

        let rejected_ids = [rejected_a, rejected_b].map(|event| match event {
            OrderEventAny::Rejected(e) => {
                assert!(
                    e.reason
                        .as_str()
                        .contains("Lighter submit_order_list dispatch failed"),
                );
                e.client_order_id
            }
            other => panic!("expected Rejected, was {other:?}"),
        });

        assert!(rejected_ids.contains(&order_a.client_order_id()));
        assert!(rejected_ids.contains(&order_b.client_order_id()));
        assert!(client.dispatch.cloid_map.get(&index_a).is_none());
        assert!(client.dispatch.cloid_map.get(&index_b).is_none());
        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
    }

    #[tokio::test]
    async fn submit_order_list_over_max_batch_size_denies_all_without_dispatch() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let mut orders = Vec::new();

        for i in 0..=LIGHTER_MAX_BATCH_TX {
            let order = test_limit_order(&mut factory, instrument_id, &format!("O-LIST-MAX-{i}"));
            cache_order(&cache, order.clone());
            orders.push(order);
        }

        let command = submit_order_list_command(&orders, "OL-MAX");
        client.submit_order_list(command).unwrap();

        for order in &orders {
            match recv_order_event(&mut rx).await {
                OrderEventAny::Denied(e) => {
                    assert_eq!(e.client_order_id, order.client_order_id());
                    assert!(
                        e.reason
                            .as_str()
                            .contains("sendTxBatch supports at most 15 txs"),
                    );
                }
                other => panic!("expected Denied, was {other:?}"),
            }
        }

        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "max-size denial must not emit extra events",
        );
        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
    }

    #[tokio::test]
    async fn submit_order_list_denies_unsupported_order_and_dispatches_supported() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let valid = test_limit_order(&mut factory, instrument_id, "O-LIST-VALID");
        let unsupported = factory.limit(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("0.1000"),
            Price::from("2361.31"),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            Some(Quantity::from("0.0500")),
            None,
            None,
            None,
            None,
            None,
            Some(ClientOrderId::from("O-LIST-ICEBERG")),
        );
        let unsupported_index = client
            .dispatch
            .derive_client_order_index(&unsupported.client_order_id());
        cache_order(&cache, valid.clone());
        cache_order(&cache, unsupported.clone());

        let command =
            submit_order_list_command(&[unsupported.clone(), valid.clone()], "OL-PARTIAL");
        client.submit_order_list(command).unwrap();

        match recv_order_event(&mut rx).await {
            OrderEventAny::Denied(e) => {
                assert_eq!(e.client_order_id, unsupported.client_order_id());
                assert!(e.reason.as_str().contains("display_qty"));
            }
            other => panic!("expected Denied, was {other:?}"),
        }

        match recv_order_event(&mut rx).await {
            OrderEventAny::Submitted(e) => assert_eq!(e.client_order_id, valid.client_order_id()),
            other => panic!("expected Submitted, was {other:?}"),
        }

        match recv_order_event(&mut rx).await {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, valid.client_order_id());
                assert!(
                    e.reason
                        .as_str()
                        .contains("Lighter submit_order_list dispatch failed"),
                );
            }
            other => panic!("expected Rejected, was {other:?}"),
        }

        assert!(client.dispatch.cloid_map.get(&unsupported_index).is_none());
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_order_list_grouped_contingency_denies_all_without_dispatch() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let order_a = test_contingent_limit_order(instrument_id, "O-LIST-OCO-A", "OL-OCO");
        let order_b = test_contingent_limit_order(instrument_id, "O-LIST-OCO-B", "OL-OCO");
        cache_order(&cache, order_a.clone());
        cache_order(&cache, order_b.clone());

        let command = submit_order_list_command(&[order_a.clone(), order_b.clone()], "OL-OCO");
        client.submit_order_list(command).unwrap();

        for order in [&order_a, &order_b] {
            match recv_order_event(&mut rx).await {
                OrderEventAny::Denied(e) => {
                    assert_eq!(e.client_order_id, order.client_order_id());
                    assert!(e.reason.as_str().contains("supports only independent"));
                }
                other => panic!("expected Denied, was {other:?}"),
            }
        }

        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "grouped list denial must not emit extra events",
        );
        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
    }

    #[tokio::test]
    async fn cancel_order_send_failure_emits_cancel_rejected_and_rolls_back() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-CANCEL-FAIL");
        let client_order_id = order.client_order_id();
        let venue_order_id = VenueOrderId::from("123");
        cache_pending_cancel_order(&cache, order, venue_order_id);

        let command = CancelOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.cancel_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;

        match rejected {
            OrderEventAny::CancelRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter cancel_order dispatch failed"),
                );
                assert!(event.reason.as_str().contains("handler unavailable"));
            }
            event => panic!("expected cancel rejected event, was {event:?}"),
        }

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            0,
            "local-send-failure must remove the pending cancel entry",
        );
    }

    #[tokio::test]
    async fn cancel_order_prepare_failure_emits_cancel_rejected_without_dispatch() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-CANCEL-NO-VOI");
        let client_order_id = order.client_order_id();
        cache_pending_cancel_order(&cache, order, VenueOrderId::from("123"));

        let command = CancelOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.cancel_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::CancelRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.venue_order_id, None);
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter cancel_order failed")
                );
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("venue order_id not yet known")
                );
            }
            event => panic!("expected cancel rejected event, was {event:?}"),
        }

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "prepare failure must emit exactly one cancel rejection",
        );
    }

    #[tokio::test]
    async fn cancel_order_prepare_failure_emits_cancel_rejected_when_order_uncached() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("O-CANCEL-NO-CACHE");
        let venue_order_id = VenueOrderId::from("123");

        let command = CancelOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.cancel_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::CancelRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter cancel_order failed")
                );
                assert!(event.reason.as_str().contains("order not found in cache"));
            }
            event => panic!("expected cancel rejected event, was {event:?}"),
        }

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "prepare failure must emit exactly one cancel rejection",
        );
    }

    #[tokio::test]
    async fn cancel_all_orders_prepare_failure_suppresses_cancel_rejected_for_open_order() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-CANCEL-ALL-NO-VOI");
        cache_accepted_order(&cache, order, VenueOrderId::from("123"));

        let command = CancelAllOrders::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            OrderSide::Buy,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.cancel_all_orders(command).unwrap();

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "cancel-all prepare failure must not emit an invalid cancel rejection",
        );
    }

    #[tokio::test]
    async fn cancel_order_nonce_prepare_failure_emits_cancel_rejected() {
        let mut config = test_config();
        config.base_url_http = Some(spawn_next_nonce_server(100).await);
        let (client, cache, mut rx) = create_execution_client_with_config(config);
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-CANCEL-NONCE-FAIL");
        let client_order_id = order.client_order_id();
        let venue_order_id = VenueOrderId::from("123");
        cache_pending_cancel_order(&cache, order, venue_order_id);

        let window = i64::from(client.dispatch.nonce_manager.skip_window());
        for _ in 0..window {
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
        }

        let command = CancelOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.cancel_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::CancelRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("failed to allocate Lighter nonce"),
                );
                assert!(event.reason.as_str().contains("skip-window exhausted"));
            }
            event => panic!("expected cancel rejected event, was {event:?}"),
        }

        wait_for_spawned_tasks(&client).await;
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            100,
        );
    }

    #[tokio::test]
    async fn batch_cancel_orders_send_failure_emits_rejected_per_cancel_and_rolls_back() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let cancels = ["O-BATCH-CANCEL-A", "O-BATCH-CANCEL-B"]
            .iter()
            .enumerate()
            .map(|(i, id)| {
                let order = test_limit_order(&mut factory, instrument_id, id);
                let client_order_id = order.client_order_id();
                let venue_order_id = VenueOrderId::from(format!("{}", 123 + i).as_str());
                cache_pending_cancel_order(&cache, order, venue_order_id);

                CancelOrder::new(
                    trader_id(),
                    Some(client_id()),
                    strategy_id(),
                    instrument_id,
                    client_order_id,
                    Some(venue_order_id),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();

        let command = BatchCancelOrders::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            cancels.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.batch_cancel_orders(command).unwrap();

        let first = recv_order_event(&mut rx).await;
        let second = recv_order_event(&mut rx).await;
        let rejected_ids = [first, second].map(|event| match event {
            OrderEventAny::CancelRejected(e) => {
                assert!(
                    e.reason
                        .as_str()
                        .contains("Lighter batch_cancel_orders dispatch failed"),
                );
                e.client_order_id
            }
            other => panic!("expected CancelRejected, was {other:?}"),
        });

        for cancel in cancels {
            assert!(rejected_ids.contains(&cancel.client_order_id));
        }
        assert_nonce_reusable(&client.dispatch);
        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            0,
            "local-send-failure must remove batch cancel pending entries",
        );
    }

    #[tokio::test]
    async fn batch_cancel_orders_over_max_batch_size_rejects_each_cancel_without_dispatch() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let cancels = (0..=LIGHTER_MAX_BATCH_TX)
            .map(|i| {
                CancelOrder::new(
                    trader_id(),
                    Some(client_id()),
                    strategy_id(),
                    instrument_id,
                    ClientOrderId::from(format!("O-BATCH-CANCEL-MAX-{i}").as_str()),
                    Some(VenueOrderId::from(format!("{}", 1_000 + i).as_str())),
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                )
            })
            .collect::<Vec<_>>();

        let command = BatchCancelOrders::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            cancels.clone(),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.batch_cancel_orders(command).unwrap();

        for cancel in &cancels {
            match recv_order_event(&mut rx).await {
                OrderEventAny::CancelRejected(e) => {
                    assert_eq!(e.client_order_id, cancel.client_order_id);
                    assert!(
                        e.reason
                            .as_str()
                            .contains("sendTxBatch supports at most 15 txs"),
                    );
                }
                other => panic!("expected CancelRejected, was {other:?}"),
            }
        }
        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
    }

    #[rstest]
    fn cancel_order_from_cancel_all_preserves_tracing_ids() {
        let instrument_id = InstrumentId::from("ETH-PERP.LIGHTER");
        let client_order_id = ClientOrderId::from("O-CANCEL-ALL-CHILD");
        let command_id = UUID4::new();
        let correlation_id = UUID4::new();
        let causation_id = UUID4::new();
        let ts_init = UnixNanos::default();
        let mut cmd = CancelAllOrders::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            OrderSide::Buy,
            command_id,
            ts_init,
            None,
            Some(correlation_id),
        );
        cmd.causation_id = Some(causation_id);

        let order_cmd = cancel_order_from_cancel_all(&cmd, client_order_id);

        assert_eq!(order_cmd.trader_id, trader_id());
        assert_eq!(order_cmd.client_id, Some(client_id()));
        assert_eq!(order_cmd.strategy_id, strategy_id());
        assert_eq!(order_cmd.instrument_id, instrument_id);
        assert_eq!(order_cmd.client_order_id, client_order_id);
        assert_eq!(order_cmd.venue_order_id, None);
        assert_eq!(order_cmd.command_id, command_id);
        assert_eq!(order_cmd.ts_init, ts_init);
        assert_eq!(order_cmd.params, None);
        assert_eq!(order_cmd.correlation_id, Some(correlation_id));
        assert_eq!(order_cmd.causation_id, Some(causation_id));
    }

    #[tokio::test]
    async fn modify_order_send_failure_emits_modify_rejected_and_rolls_back() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-MODIFY-FAIL");
        let client_order_id = order.client_order_id();
        let venue_order_id = VenueOrderId::from("123");
        cache_order(&cache, order);

        let command = ModifyOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            Some(Quantity::from("0.2000")),
            Some(Price::from("2362.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.modify_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;

        match rejected {
            OrderEventAny::ModifyRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter modify_order dispatch failed"),
                );
                assert!(event.reason.as_str().contains("handler unavailable"));
            }
            event => panic!("expected modify rejected event, was {event:?}"),
        }

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            0,
            "local-send-failure must remove the pending modify entry",
        );
    }

    #[tokio::test]
    async fn modify_order_prepare_failure_emits_modify_rejected_without_dispatch() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("O-MODIFY-NO-CACHE");
        let venue_order_id = VenueOrderId::from("123");

        let command = ModifyOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            Some(Quantity::from("0.2000")),
            Some(Price::from("2362.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.modify_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::ModifyRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter modify_order failed")
                );
                assert!(event.reason.as_str().contains("order not found in cache"));
            }
            event => panic!("expected modify rejected event, was {event:?}"),
        }

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "prepare failure must emit exactly one modify rejection",
        );
    }

    #[tokio::test]
    async fn modify_order_prepare_failure_emits_modify_rejected_when_instrument_uncached() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id =
            client
                .registry
                .insert(TEST_MARKET_INDEX, "ETH", LighterProductType::Perp);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-MODIFY-NO-INSTRUMENT");
        let client_order_id = order.client_order_id();
        let venue_order_id = VenueOrderId::from("123");
        cache_order(&cache, order);

        let command = ModifyOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            Some(Quantity::from("0.2000")),
            Some(Price::from("2362.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.modify_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::ModifyRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.instrument_id, instrument_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter modify_order failed")
                );
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("instrument not found in cache")
                );
            }
            event => panic!("expected modify rejected event, was {event:?}"),
        }

        assert_nonce_reusable(&client.dispatch);
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "prepare failure must emit exactly one modify rejection",
        );
    }

    #[tokio::test]
    async fn modify_order_nonce_prepare_failure_emits_modify_rejected() {
        let mut config = test_config();
        config.base_url_http = Some(spawn_next_nonce_server(101).await);
        let (client, cache, mut rx) = create_execution_client_with_config(config);
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "O-MODIFY-NONCE-FAIL");
        let client_order_id = order.client_order_id();
        let venue_order_id = VenueOrderId::from("123");
        cache_order(&cache, order);

        let window = i64::from(client.dispatch.nonce_manager.skip_window());
        for _ in 0..window {
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
        }

        let command = ModifyOrder::new(
            trader_id(),
            Some(client_id()),
            strategy_id(),
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            Some(Quantity::from("0.2000")),
            Some(Price::from("2362.00")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );
        client.modify_order(command).unwrap();

        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::ModifyRejected(event) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.venue_order_id, Some(venue_order_id));
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("failed to allocate Lighter nonce"),
                );
                assert!(event.reason.as_str().contains("skip-window exhausted"));
            }
            event => panic!("expected modify rejected event, was {event:?}"),
        }

        wait_for_spawned_tasks(&client).await;
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            101,
        );
    }

    #[tokio::test]
    async fn update_leverage_requires_credentials() {
        let (mut client, _cache, _rx) = create_execution_client();
        client.credential = None;
        let instrument_id = InstrumentId::from("ETH-PERP.LIGHTER");

        let err = client
            .update_leverage(instrument_id, 500, LighterPositionMarginMode::Isolated)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("cannot update leverage without credentials"),
        );
    }

    #[tokio::test]
    async fn update_leverage_requires_registered_instrument() {
        let (client, _cache, _rx) = create_execution_client();
        let unknown = InstrumentId::from("DOGE-PERP.LIGHTER");

        let err = client
            .update_leverage(unknown, 500, LighterPositionMarginMode::Isolated)
            .unwrap_err();

        assert!(
            err.to_string()
                .contains("no Lighter market_index registered")
        );
        // Pin that nonce was not burned on the rejected path: instrument
        // lookup must happen before `build_tx_context` allocates a nonce.
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn update_leverage_dispatches_and_rolls_back_on_send_failure() {
        let (client, cache, _rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);

        client
            .update_leverage(instrument_id, 500, LighterPositionMarginMode::Isolated)
            .unwrap();

        wait_for_spawned_tasks(&client).await;
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn update_leverage_rejects_zero_margin_fraction() {
        let (client, cache, _rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);

        let err = client
            .update_leverage(instrument_id, 0, LighterPositionMarginMode::Cross)
            .unwrap_err();
        assert!(err.to_string().contains("must be in 1..=10_000"));
    }

    #[tokio::test]
    async fn update_leverage_rejects_above_margin_fraction_tick() {
        let (client, cache, _rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);

        let err = client
            .update_leverage(instrument_id, 10_001, LighterPositionMarginMode::Cross)
            .unwrap_err();
        assert!(err.to_string().contains("must be in 1..=10_000"));
    }

    #[tokio::test]
    async fn update_leverage_accepts_minimum_margin_fraction() {
        // Pin the inclusive lower bound of the venue's `MarginFractionTick`
        // range. An exclusive `(1.., ...)` range check would fail this case.
        let (client, cache, _rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);

        client
            .update_leverage(instrument_id, 1, LighterPositionMarginMode::Cross)
            .unwrap();

        wait_for_spawned_tasks(&client).await;
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn update_leverage_accepts_maximum_margin_fraction() {
        // Pin the inclusive upper bound of the venue's `MarginFractionTick`
        // range. An exclusive `(..10_000)` range check would fail this case.
        let (client, cache, _rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);

        client
            .update_leverage(instrument_id, 10_000, LighterPositionMarginMode::Isolated)
            .unwrap();

        wait_for_spawned_tasks(&client).await;
        assert_nonce_reusable(&client.dispatch);
    }

    async fn wait_for_spawned_tasks(client: &LighterExecutionClient) {
        wait_until_async(
            || async { client.pending_tasks_all_finished() },
            Duration::from_secs(2),
        )
        .await;
    }

    fn mark_all_streams_ready(client: &LighterExecutionClient) {
        let ready = &client.dispatch.account_streams_ready;
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();
        ready.mark_assets();
        ready.mark_user_stats();
    }

    #[tokio::test]
    async fn await_account_streams_ready_times_out_when_no_frame_arrives() {
        // Drives the timeout branch `connect()` uses to tear the WS down
        // when at least one account stream has not delivered a first frame.
        let (client, _cache, _rx) = create_execution_client();

        let err = client.await_account_streams_ready(0.05).await.unwrap_err();

        assert!(
            err.to_string().contains("Timeout")
                && err.to_string().contains("Lighter account streams"),
            "unexpected error message, was {err}",
        );
    }

    #[tokio::test]
    async fn await_account_streams_ready_returns_when_all_streams_marked() {
        let (client, _cache, _rx) = create_execution_client();
        mark_all_streams_ready(&client);

        client.await_account_streams_ready(0.05).await.unwrap();
    }

    #[tokio::test]
    async fn await_account_streams_ready_returns_when_streams_arrive_mid_wait() {
        // Pins that the Notify-based wait wakes promptly when frames land
        // after the wait has started.
        let (client, _cache, _rx) = create_execution_client();
        let ready = Arc::clone(&client.dispatch.account_streams_ready);

        let wait = client.await_account_streams_ready(1.0);
        let seed = async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            ready.mark_orders();
            ready.mark_trades();
            ready.mark_positions();
            ready.mark_assets();
            ready.mark_user_stats();
        };

        let (result, ()) = tokio::join!(wait, seed);
        result.unwrap();
    }

    #[tokio::test]
    async fn await_account_streams_ready_times_out_with_partial_marks() {
        // Three out of four streams marked must still time out: strict
        // await means every account stream has to deliver before connect
        // unblocks.
        let (client, _cache, _rx) = create_execution_client();
        let ready = &client.dispatch.account_streams_ready;
        ready.mark_orders();
        ready.mark_trades();
        ready.mark_positions();

        let err = client.await_account_streams_ready(0.05).await.unwrap_err();
        assert!(
            err.to_string().contains("assets"),
            "pending list should call out the missing stream, was {err}",
        );
    }

    #[tokio::test]
    async fn await_account_streams_ready_after_reset_requires_new_marks() {
        // Pins the connect-retry contract: marks from a prior session
        // must not satisfy a fresh await once `reset()` has cleared the
        // gate. A regression that drops the reset() call from connect()
        // would let a retried session return immediately with stale flags.
        let (client, _cache, _rx) = create_execution_client();
        mark_all_streams_ready(&client);
        client.await_account_streams_ready(0.05).await.unwrap();

        client.dispatch.account_streams_ready.reset();

        let err = client.await_account_streams_ready(0.05).await.unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("orders"), "pending list missing orders: {msg}");
        assert!(msg.contains("trades"), "pending list missing trades: {msg}");
        assert!(
            msg.contains("positions"),
            "pending list missing positions: {msg}",
        );
        assert!(msg.contains("assets"), "pending list missing assets: {msg}");
        assert!(
            msg.contains("user_stats"),
            "pending list missing user_stats: {msg}",
        );
    }

    fn test_market_order(
        factory: &mut OrderFactory,
        instrument_id: InstrumentId,
        client_order_id: &str,
        side: OrderSide,
    ) -> OrderAny {
        factory.market(
            instrument_id,
            side,
            Quantity::from("0.1000"),
            Some(TimeInForce::Ioc),
            Some(false),
            Some(false),
            None,
            None,
            None,
            Some(ClientOrderId::from(client_order_id)),
        )
    }

    fn add_test_quote(
        cache: &Rc<RefCell<Cache>>,
        instrument_id: InstrumentId,
        bid: &str,
        ask: &str,
    ) {
        let quote = QuoteTick::new(
            instrument_id,
            Price::from(bid),
            Price::from(ask),
            Quantity::from("1.0000"),
            Quantity::from("1.0000"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        cache.borrow_mut().add_quote(quote).unwrap();
    }

    #[tokio::test]
    async fn submit_market_order_without_cached_quote_emits_denied() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_market_order(
            &mut factory,
            instrument_id,
            "O-MARKET-NO-QUOTE",
            OrderSide::Buy,
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        // submit_order returns Err but also emits OrderDenied; consume both.
        let _ = client.submit_order(command);

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Denied(event) => {
                assert!(
                    event.reason.as_str().contains("no cached quote"),
                    "expected no-cached-quote in reason, was {:?}",
                    event.reason,
                );
            }
            event => panic!("expected denied event, was {event:?}"),
        }
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_market_buy_with_quote_uses_ask_widened_by_slippage() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        add_test_quote(&cache, instrument_id, "2360.00", "2361.00");

        let mut factory = test_order_factory();
        let order = test_market_order(
            &mut factory,
            instrument_id,
            "O-MARKET-QUOTED-BUY",
            OrderSide::Buy,
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        let _ = client.submit_order(command);

        let submitted = recv_order_event(&mut rx).await;
        assert!(
            matches!(submitted, OrderEventAny::Submitted(_)),
            "expected submitted, was {submitted:?}",
        );
        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::Rejected(event) => {
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter submit_order dispatch failed"),
                );
            }
            event => panic!("expected rejected event, was {event:?}"),
        }
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_order_with_sub_tick_quantity_emits_denied() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        // ETH-PERP size_precision=4; quantity 0.00001 truncates to 0 ticks.
        let order = factory.limit(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("0.00001"),
            Price::from("2361.31"),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(ClientOrderId::from("O-SUB-TICK-QTY")),
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        let _ = client.submit_order(command);

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Denied(event) => {
                assert!(
                    event.reason.as_str().contains("rounds to 0 ticks"),
                    "expected rounds-to-0 in reason, was {:?}",
                    event.reason,
                );
            }
            event => panic!("expected denied event, was {event:?}"),
        }
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_order_below_min_notional_emits_denied() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = factory.limit(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("0.0010"),
            Price::from("2361.31"),
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(ClientOrderId::from("O-BELOW-MIN-NOTIONAL")),
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        let _ = client.submit_order(command);

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Denied(event) => {
                assert!(
                    event.reason.as_str().contains("min_quote_amount"),
                    "expected min_quote_amount in reason, was {:?}",
                    event.reason,
                );
            }
            event => panic!("expected denied event, was {event:?}"),
        }
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_stop_market_with_sub_tick_trigger_emits_denied() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        // ETH-PERP price_precision=2; trigger 0.001 truncates to 0 ticks.
        let order = factory.stop_market(
            instrument_id,
            OrderSide::Buy,
            Quantity::from("0.1000"),
            Price::from("0.001"),
            None,
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(ClientOrderId::from("O-STOP-SUB-TICK")),
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        let _ = client.submit_order(command);

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Denied(event) => {
                assert!(
                    event.reason.as_str().contains("rounds to 0 ticks"),
                    "expected rounds-to-0 in reason, was {:?}",
                    event.reason,
                );
            }
            event => panic!("expected denied event, was {event:?}"),
        }
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_stop_market_dispatches_using_trigger_widened_by_slippage() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = factory.stop_market(
            instrument_id,
            OrderSide::Sell,
            Quantity::from("0.1000"),
            Price::from("2300.00"), // trigger
            None,
            Some(TimeInForce::Gtc),
            None,
            Some(false),
            Some(false),
            None,
            None,
            None,
            None,
            None,
            None,
            Some(ClientOrderId::from("O-STOP-MARKET")),
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        let _ = client.submit_order(command);

        let submitted = recv_order_event(&mut rx).await;
        assert!(matches!(submitted, OrderEventAny::Submitted(_)));
        let rejected = recv_order_event(&mut rx).await;
        assert!(matches!(rejected, OrderEventAny::Rejected(_)));
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn submit_market_order_respects_per_order_slippage_override() {
        // 0-bps override on a valid ask exercises the params path without
        // adding any widening.
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        add_test_quote(&cache, instrument_id, "2360.00", "2361.00");

        let mut factory = test_order_factory();
        let order = test_market_order(
            &mut factory,
            instrument_id,
            "O-MARKET-ZERO-SLIP",
            OrderSide::Buy,
        );
        cache_order(&cache, order.clone());

        let params: Params =
            serde_json::from_value(serde_json::json!({"market_order_slippage_bps": 0})).unwrap();
        let mut command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        command.params = Some(params);
        let _ = client.submit_order(command);

        let submitted = recv_order_event(&mut rx).await;
        assert!(matches!(submitted, OrderEventAny::Submitted(_)));
        let rejected = recv_order_event(&mut rx).await;
        assert!(matches!(rejected, OrderEventAny::Rejected(_)));
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn resolve_slippage_bps_prefers_params_over_config_default() {
        let (client, _cache, _rx) = create_execution_client();
        assert_eq!(client.resolve_slippage_bps(None), 50);

        let override_params: Params =
            serde_json::from_value(serde_json::json!({"market_order_slippage_bps": 100})).unwrap();
        assert_eq!(client.resolve_slippage_bps(Some(&override_params)), 100);

        let unrelated_params: Params =
            serde_json::from_value(serde_json::json!({"other_key": 999})).unwrap();
        assert_eq!(client.resolve_slippage_bps(Some(&unrelated_params)), 50);
    }

    #[tokio::test]
    async fn submit_market_sell_with_quote_uses_bid_widened_by_slippage() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        add_test_quote(&cache, instrument_id, "2360.00", "2361.00");

        let mut factory = test_order_factory();
        let order = test_market_order(
            &mut factory,
            instrument_id,
            "O-MARKET-QUOTED-SELL",
            OrderSide::Sell,
        );
        cache_order(&cache, order.clone());

        let command = SubmitOrder::from_order(
            &order,
            trader_id(),
            Some(client_id()),
            None,
            UUID4::new(),
            UnixNanos::default(),
        );
        let _ = client.submit_order(command);

        let submitted = recv_order_event(&mut rx).await;
        assert!(
            matches!(submitted, OrderEventAny::Submitted(_)),
            "expected submitted, was {submitted:?}",
        );
        let rejected = recv_order_event(&mut rx).await;
        match rejected {
            OrderEventAny::Rejected(event) => {
                assert!(
                    event
                        .reason
                        .as_str()
                        .contains("Lighter submit_order dispatch failed"),
                );
            }
            event => panic!("expected rejected event, was {event:?}"),
        }
        assert_nonce_reusable(&client.dispatch);
    }

    #[rstest]
    fn integrator_attributes_tags_nautilus_account_at_zero_fees() {
        let attrs = integrator_attributes();
        assert_eq!(
            attrs.integrator_account_index,
            LIGHTER_NAUTILUS_INTEGRATOR_ACCOUNT_INDEX,
        );
        assert_eq!(attrs.integrator_taker_fee, 0);
        assert_eq!(attrs.integrator_maker_fee, 0);
        assert_eq!(attrs.skip_nonce, 0);
    }

    use std::str::FromStr;

    use nautilus_live::ExecutionEventEmitter;
    use rust_decimal::Decimal;

    use crate::{
        common::enums::{
            LighterOrderKind, LighterOrderSide, LighterOrderStatus, LighterOrderTimeInForce,
            LighterTradeType, LighterTriggerStatus,
        },
        http::models::{LighterOrder, LighterTrade},
    };

    fn dispatcher_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            trader_id(),
            account_id(),
            AccountType::Margin,
            None,
        );
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<ExecutionEvent>();
        emitter.set_sender(sender);
        (emitter, receiver)
    }

    /// Test rig that owns a `WsDispatchState`, a process-global instrument
    /// cache entry, a `MarketRegistry`, and an emitter wired to a receiver.
    /// Used by every dispatcher test to keep the call-site short.
    struct DispatcherRig {
        dispatch: WsDispatchState,
        registry: Arc<MarketRegistry>,
        emitter: ExecutionEventEmitter,
        rx: tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        instrument_id: InstrumentId,
        cloid: ClientOrderId,
    }

    fn dispatcher_rig(cloid_suffix: &str) -> DispatcherRig {
        let registry = Arc::new(MarketRegistry::new());
        // All dispatcher tests share the same instrument (ETH-PERP) so
        // `LIGHTER_INSTRUMENT_CACHE` only ever holds one entry; per-test
        // isolation comes from the per-rig `WsDispatchState` and the
        // unique cloid built from `cloid_suffix`.
        let instrument_id = registry.insert(TEST_MARKET_INDEX, "ETH", LighterProductType::Perp);
        let instrument = InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            Symbol::new("ETH-PERP"),
            Currency::from("ETH"),
            Currency::from("USDC"),
            Currency::from("USDC"),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
            None,
            None,
            None,
            None,
            None,
            Some(Money::from("10.000000 USDC")),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ));
        LIGHTER_INSTRUMENT_CACHE.insert(instrument_id, instrument);
        let (emitter, rx) = dispatcher_emitter();
        DispatcherRig {
            dispatch: WsDispatchState::new(),
            registry,
            emitter,
            rx,
            instrument_id,
            cloid: ClientOrderId::new(format!("CLOID-{cloid_suffix}")),
        }
    }

    fn register_identity(rig: &DispatcherRig) {
        rig.dispatch.register_order_identity(
            rig.cloid,
            OrderIdentity {
                instrument_id: rig.instrument_id,
                strategy_id: strategy_id(),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );
    }

    fn dispatcher_test_order(rig: &DispatcherRig, status: LighterOrderStatus) -> LighterOrder {
        let derived = rig.dispatch.derive_client_order_index(&rig.cloid);
        rig.dispatch.register_cloid(derived, rig.cloid);

        LighterOrder {
            order_index: 281_476_929_510_110,
            client_order_index: derived,
            order_id: "281476929510110".to_string(),
            client_order_id: derived.to_string(),
            market_index: TEST_MARKET_INDEX,
            owner_account_index: TEST_ACCOUNT_INDEX_I64,
            initial_base_amount: Decimal::from_str("0.0050").unwrap(),
            price: Decimal::from_str("2352.74").unwrap(),
            nonce: 9_182_390_020,
            remaining_base_amount: Decimal::from_str("0.0050").unwrap(),
            is_ask: false,
            base_size: 50,
            base_price: 235_274,
            filled_base_amount: Decimal::ZERO,
            filled_quote_amount: Decimal::ZERO,
            side: Some(LighterOrderSide::Buy),
            order_type: LighterOrderKind::Limit,
            time_in_force: LighterOrderTimeInForce::GoodTillTime,
            reduce_only: false,
            trigger_price: Decimal::ZERO,
            order_expiry: 1_780_360_584_479,
            status,
            trigger_status: LighterTriggerStatus::Na,
            trigger_time: 0,
            parent_order_index: 0,
            parent_order_id: "0".to_string(),
            to_trigger_order_id_0: "0".to_string(),
            to_trigger_order_id_1: "0".to_string(),
            to_cancel_order_id_0: "0".to_string(),
            integrator_fee_collector_index: "0".to_string(),
            integrator_taker_fee: Decimal::ZERO,
            integrator_maker_fee: Decimal::ZERO,
            block_height: 227_535_532,
            timestamp: 1_777_941_383_576,
            created_at: 1_777_941_383_576,
            updated_at: 1_777_941_383_900,
            transaction_time: 1_777_941_383_576_735,
        }
    }

    fn dispatcher_test_trade(rig: &DispatcherRig, user_is_bidder: bool) -> LighterTrade {
        let derived = rig.dispatch.derive_client_order_index(&rig.cloid);
        rig.dispatch.register_cloid(derived, rig.cloid);
        LighterTrade {
            trade_id: 19_209_006_902,
            trade_id_str: Some("19209006902".to_string()),
            tx_hash: "000000128b1ee814".to_string(),
            trade_type: LighterTradeType::Trade,
            market_id: TEST_MARKET_INDEX,
            size: Decimal::from_str("0.1336").unwrap(),
            price: Decimal::from_str("2352.73").unwrap(),
            usd_amount: Decimal::from_str("314.324728").unwrap(),
            ask_id: 281_476_929_510_102,
            ask_id_str: Some("281476929510102".to_string()),
            bid_id: 562_947_905_631_053,
            bid_id_str: Some("562947905631053".to_string()),
            ask_client_id: if user_is_bidder { 0 } else { derived },
            ask_client_id_str: Some(if user_is_bidder {
                "0".to_string()
            } else {
                derived.to_string()
            }),
            bid_client_id: if user_is_bidder { derived } else { 0 },
            bid_client_id_str: Some(if user_is_bidder {
                derived.to_string()
            } else {
                "0".to_string()
            }),
            ask_account_id: if user_is_bidder {
                91_249
            } else {
                TEST_ACCOUNT_INDEX_I64
            },
            bid_account_id: if user_is_bidder {
                TEST_ACCOUNT_INDEX_I64
            } else {
                91_249
            },
            is_maker_ask: false,
            block_height: 227_535_535,
            timestamp: 1_777_941_384_181,
            taker_fee: Some(196),
            taker_position_size_before: None,
            taker_entry_quote_before: None,
            taker_initial_margin_fraction_before: None,
            taker_position_sign_changed: None,
            maker_fee: Some(28),
            maker_position_size_before: None,
            maker_entry_quote_before: None,
            maker_initial_margin_fraction_before: None,
            maker_position_sign_changed: None,
            transaction_time: 1_777_941_384_181_586,
            ask_account_pnl: None,
            bid_account_pnl: None,
        }
    }

    /// Drain all pending events from the rig's receiver. Useful when a
    /// test wants to assert what landed without timing-sensitive
    /// `recv_order_event` waits.
    fn drain_events(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<ExecutionEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    #[rstest]
    fn dispatch_lighter_order_tracked_emits_accepted_then_silent_repeat() {
        let mut rig = dispatcher_rig("1");
        register_identity(&rig);
        let order = dispatcher_test_order(&rig, LighterOrderStatus::Open);

        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(1),
        );
        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(2),
        );

        let events = drain_events(&mut rig.rx);
        assert_eq!(
            events.len(),
            1,
            "exactly one event expected, was {events:?}",
        );

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Accepted(e)) => {
                assert_eq!(e.client_order_id, rig.cloid);
                assert_eq!(e.venue_order_id.to_string(), "281476929510110");
            }
            other => panic!("expected Accepted, was {other:?}"),
        }
        assert!(rig.dispatch.accepted_was_emitted(&rig.cloid));
        assert!(rig.dispatch.snapshot_for(&rig.cloid).is_some());
    }

    #[rstest]
    fn dispatch_lighter_order_tracked_emits_updated_on_shape_change() {
        let mut rig = dispatcher_rig("2");
        register_identity(&rig);
        let order = dispatcher_test_order(&rig, LighterOrderStatus::Open);

        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(1),
        );
        assert_eq!(drain_events(&mut rig.rx).len(), 1);

        let mut modified = order;
        modified.price = Decimal::from_str("2400.00").unwrap();

        dispatch_lighter_order(
            &modified,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(2),
        );

        let events = drain_events(&mut rig.rx);
        assert_eq!(
            events.len(),
            1,
            "expected one Updated event, was {events:?}",
        );

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Updated(e)) => {
                assert_eq!(e.client_order_id, rig.cloid);
                assert_eq!(e.price, Some(Price::from("2400.00")));
            }
            other => panic!("expected Updated, was {other:?}"),
        }
        let snapshot = rig.dispatch.snapshot_for(&rig.cloid).expect("snapshot");
        assert_eq!(snapshot.price, Some(Price::from("2400.00")));
    }

    #[rstest]
    fn dispatch_lighter_order_untracked_emits_report() {
        let mut rig = dispatcher_rig("3");
        // No identity registered: this is an external order.
        let mut order = dispatcher_test_order(&rig, LighterOrderStatus::Open);
        order.client_order_id = "external-1".to_string();
        order.client_order_index = 0;

        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(1),
        );

        let events = drain_events(&mut rig.rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            ExecutionEvent::Report(report) => match report {
                EngineExecutionReport::Order(r) => {
                    assert_eq!(r.venue_order_id.to_string(), "281476929510110");
                }
                other => panic!("expected order report, was {other:?}"),
            },
            other => panic!("expected report, was {other:?}"),
        }
        assert!(
            !rig.dispatch
                .accepted_was_emitted(&ClientOrderId::new("external-1"))
        );
    }

    #[rstest]
    fn dispatch_lighter_trade_tracked_synthesizes_accepted_before_filled() {
        // Fill-before-open: the trade arrives before the matching Open
        // frame. The dispatcher must synthesise `OrderAccepted` first so
        // the engine sees the lifecycle in order.
        let mut rig = dispatcher_rig("4");
        register_identity(&rig);

        let trade = dispatcher_test_trade(&rig, true);

        dispatch_lighter_trade(
            &trade,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1),
        );

        let events = drain_events(&mut rig.rx);
        assert_eq!(
            events.len(),
            2,
            "expected Accepted then Filled, was {events:?}",
        );

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Accepted(_)) => {}
            other => panic!("first event should be Accepted, was {other:?}"),
        }

        match &events[1] {
            ExecutionEvent::Order(OrderEventAny::Filled(e)) => {
                assert_eq!(e.client_order_id, rig.cloid);
                assert_eq!(e.last_qty, Quantity::from("0.1336"));
                assert_eq!(e.last_px, Price::from("2352.73"));
            }
            other => panic!("second event should be Filled, was {other:?}"),
        }
        assert!(rig.dispatch.accepted_was_emitted(&rig.cloid));
    }

    #[rstest]
    fn dispatch_lighter_trade_dedupes_repeated_trade_ids() {
        let mut rig = dispatcher_rig("5");
        register_identity(&rig);
        let trade = dispatcher_test_trade(&rig, true);

        for _ in 0..3 {
            dispatch_lighter_trade(
                &trade,
                &rig.dispatch,
                &rig.emitter,
                &rig.registry,
                account_id(),
                trader_id(),
                Some(TEST_ACCOUNT_INDEX_I64),
                UnixNanos::from(1),
            );
        }

        let events = drain_events(&mut rig.rx);
        // First call: Accepted + Filled. Subsequent calls deduped by trade_id.
        assert_eq!(
            events.len(),
            2,
            "expected dedup after first dispatch, was {events:?}"
        );
    }

    #[rstest]
    fn dispatch_tracked_order_event_terminal_cancel_removes_identity_and_snapshot() {
        let mut rig = dispatcher_rig("6");
        register_identity(&rig);
        rig.dispatch.mark_accepted_emitted(rig.cloid);
        rig.dispatch.store_snapshot(
            rig.cloid,
            crate::websocket::dispatch::OrderShapeSnapshot {
                quantity: Quantity::from("0.0050"),
                price: Some(Price::from("2352.74")),
                trigger_price: None,
            },
        );

        let mut order = dispatcher_test_order(&rig, LighterOrderStatus::Canceled);
        order.filled_base_amount = Decimal::ZERO;
        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(1),
        );

        let events = drain_events(&mut rig.rx);
        let canceled = events
            .iter()
            .find(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Canceled(_))))
            .expect("expected a Canceled event");
        if let ExecutionEvent::Order(OrderEventAny::Canceled(e)) = canceled {
            assert_eq!(e.client_order_id, rig.cloid);
        }
        assert!(!rig.dispatch.order_identities.contains_key(&rig.cloid));
        assert!(rig.dispatch.snapshot_for(&rig.cloid).is_none());
        assert!(!rig.dispatch.accepted_was_emitted(&rig.cloid));
    }

    #[rstest]
    fn dispatch_tracked_cancel_after_report_seed_skips_synthesized_accept() {
        let mut rig = dispatcher_rig("10");
        register_identity(&rig);

        let report_order = dispatcher_test_order(&rig, LighterOrderStatus::Open);
        let instrument = LIGHTER_INSTRUMENT_CACHE
            .get(&rig.instrument_id)
            .expect("instrument cached");
        let report = parse_ws_order_status_report(
            &report_order,
            instrument.value(),
            account_id(),
            UnixNanos::from(1),
        )
        .map(|report| translate_order_cloid(report, &rig.dispatch.cloid_map))
        .expect("report parses");

        assert_eq!(report.order_status, OrderStatus::Accepted);
        rig.dispatch.seed_accepted_from_report(&report);

        let mut cancel_order = dispatcher_test_order(&rig, LighterOrderStatus::Canceled);
        cancel_order.filled_base_amount = Decimal::ZERO;
        dispatch_lighter_order(
            &cancel_order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(2),
        );

        let events = drain_events(&mut rig.rx);
        assert_eq!(
            events.len(),
            1,
            "report-seeded cancel should emit only Canceled, was {events:?}",
        );
        assert!(
            !events
                .iter()
                .any(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_)))),
            "typed cancel must not synthesize a second Accepted",
        );

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Canceled(e)) => {
                assert_eq!(e.client_order_id, rig.cloid);
                assert_eq!(e.venue_order_id, Some(VenueOrderId::new("281476929510110")));
            }
            other => panic!("expected Canceled, was {other:?}"),
        }
    }

    #[rstest]
    fn dispatch_tracked_cancel_after_submitted_report_seed_skips_synthesized_accept() {
        let mut rig = dispatcher_rig("11");
        register_identity(&rig);

        let report_order = dispatcher_test_order(&rig, LighterOrderStatus::Open);
        let instrument = LIGHTER_INSTRUMENT_CACHE
            .get(&rig.instrument_id)
            .expect("instrument cached");
        let mut report = parse_ws_order_status_report(
            &report_order,
            instrument.value(),
            account_id(),
            UnixNanos::from(1),
        )
        .map(|report| translate_order_cloid(report, &rig.dispatch.cloid_map))
        .expect("report parses");
        report.order_status = OrderStatus::Submitted;

        rig.dispatch.seed_accepted_from_report(&report);
        assert!(rig.dispatch.accepted_was_emitted(&rig.cloid));

        let mut cancel_order = dispatcher_test_order(&rig, LighterOrderStatus::Canceled);
        cancel_order.filled_base_amount = Decimal::ZERO;

        dispatch_lighter_order(
            &cancel_order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(2),
        );

        let events = drain_events(&mut rig.rx);
        assert_eq!(
            events.len(),
            1,
            "Cancel after Submitted report should emit only Canceled, was {events:?}",
        );

        match &events[0] {
            ExecutionEvent::Order(OrderEventAny::Canceled(e)) => {
                assert_eq!(e.client_order_id, rig.cloid);
                assert_eq!(e.venue_order_id, Some(VenueOrderId::new("281476929510110")));
            }
            other => panic!("expected Canceled, was {other:?}"),
        }
    }

    #[rstest]
    fn dispatch_tracked_order_event_accept_dedup_is_idempotent() {
        let mut rig = dispatcher_rig("7");
        register_identity(&rig);
        let order = dispatcher_test_order(&rig, LighterOrderStatus::Open);

        // First dispatch emits Accepted. Second dispatch must be silent
        // (no shape change) and not re-emit Accepted.
        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(1),
        );
        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(2),
        );

        let events = drain_events(&mut rig.rx);
        let accepted_count = events
            .iter()
            .filter(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Accepted(_))))
            .count();
        assert_eq!(accepted_count, 1, "Accepted must be emitted exactly once");
    }

    #[rstest]
    fn dispatch_lighter_order_drops_when_instrument_uncached() {
        // Construct a rig but use a market_index the registry does not know.
        let registry = Arc::new(MarketRegistry::new());
        let (emitter, mut rx) = dispatcher_emitter();
        let dispatch = WsDispatchState::new();
        let cloid = ClientOrderId::new("CLOID-MISSING");
        dispatch.register_order_identity(
            cloid,
            OrderIdentity {
                instrument_id: InstrumentId::from("MISSING-PERP.LIGHTER"),
                strategy_id: strategy_id(),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );
        let mut order = LighterOrder {
            order_index: 1,
            client_order_index: 1,
            order_id: "1".to_string(),
            client_order_id: "1".to_string(),
            market_index: 999, // not in registry
            owner_account_index: TEST_ACCOUNT_INDEX_I64,
            initial_base_amount: Decimal::ZERO,
            price: Decimal::ZERO,
            nonce: 0,
            remaining_base_amount: Decimal::ZERO,
            is_ask: false,
            base_size: 0,
            base_price: 0,
            filled_base_amount: Decimal::ZERO,
            filled_quote_amount: Decimal::ZERO,
            side: Some(LighterOrderSide::Buy),
            order_type: LighterOrderKind::Limit,
            time_in_force: LighterOrderTimeInForce::GoodTillTime,
            reduce_only: false,
            trigger_price: Decimal::ZERO,
            order_expiry: 0,
            status: LighterOrderStatus::Open,
            trigger_status: LighterTriggerStatus::Na,
            trigger_time: 0,
            parent_order_index: 0,
            parent_order_id: "0".to_string(),
            to_trigger_order_id_0: "0".to_string(),
            to_trigger_order_id_1: "0".to_string(),
            to_cancel_order_id_0: "0".to_string(),
            integrator_fee_collector_index: "0".to_string(),
            integrator_taker_fee: Decimal::ZERO,
            integrator_maker_fee: Decimal::ZERO,
            block_height: 0,
            timestamp: 0,
            created_at: 0,
            updated_at: 0,
            transaction_time: 0,
        };
        order.client_order_id = "1".to_string();

        dispatch_lighter_order(
            &order,
            &dispatch,
            &emitter,
            &registry,
            account_id(),
            trader_id(),
            UnixNanos::from(1),
        );

        let events = drain_events(&mut rx);
        assert!(
            events.is_empty(),
            "no event for uncached instrument, was {events:?}"
        );
        assert!(dispatch.order_identities.contains_key(&cloid));
    }

    #[rstest]
    fn dispatch_lighter_trade_filters_non_account_trades_defensively() {
        let mut rig = dispatcher_rig("8");
        // Trade involves accounts 91249 and 91250, but the supplied
        // account_index is TEST_ACCOUNT_INDEX_I64 (12345). The handler is
        // the first defensive filter; this verifies the dispatcher path
        // also drops foreign trades cleanly.
        let mut trade = dispatcher_test_trade(&rig, true);
        trade.bid_account_id = 91_249;
        trade.ask_account_id = 91_250;

        dispatch_lighter_trade(
            &trade,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1),
        );

        let events = drain_events(&mut rig.rx);
        assert!(
            events.is_empty(),
            "foreign trade must produce no event, was {events:?}"
        );
        assert!(
            !rig.dispatch
                .seen_trade_ids
                .contains(&TradeId::new("19209006902"),)
        );
    }

    #[rstest]
    fn register_cloid_in_submit_path_uses_probed_index_on_collision() {
        // Forcing a collision at the derived index for a fresh cloid must
        // result in `register_cloid` returning a different (probed) index;
        // the submit path uses this returned value as the venue-side
        // client_order_index, so it must be the probed one.
        let dispatch = WsDispatchState::new();
        let cloid = ClientOrderId::new("PROBE-CLOID");
        let derived = dispatch.derive_client_order_index(&cloid);

        let intruder = ClientOrderId::new("INTRUDER");
        dispatch.cloid_map.insert(derived, intruder);

        let chosen = dispatch.register_cloid(derived, cloid);

        assert_ne!(chosen, derived);
        assert_eq!(
            dispatch.cloid_map.get(&derived).map(|e| *e.value()),
            Some(intruder),
        );
        assert_eq!(
            dispatch.cloid_map.get(&chosen).map(|e| *e.value()),
            Some(cloid),
        );
    }

    #[rstest]
    fn dispatch_lighter_order_seeds_snapshot_after_synthesized_accept() {
        // After a synthesised `OrderAccepted` (fill-before-open), the
        // next `Open` frame must seed the shape snapshot even when the
        // parser returns None. Without the seed, shape_changed stays
        // permanently false and a later modify is lost.
        let mut rig = dispatcher_rig("9");
        register_identity(&rig);

        let trade = dispatcher_test_trade(&rig, true);
        dispatch_lighter_trade(
            &trade,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1),
        );
        assert_eq!(drain_events(&mut rig.rx).len(), 2);
        assert!(rig.dispatch.accepted_was_emitted(&rig.cloid));
        assert!(
            rig.dispatch.snapshot_for(&rig.cloid).is_none(),
            "synthesised Accept has no snapshot until the Open frame seeds one",
        );

        // Open frame lands later (matches venue ordering). Parser
        // returns None (already accepted, shape unchanged) but the
        // dispatcher must still seed the snapshot baseline.
        let order = dispatcher_test_order(&rig, LighterOrderStatus::Open);
        dispatch_lighter_order(
            &order,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(2),
        );
        assert!(
            rig.dispatch.snapshot_for(&rig.cloid).is_some(),
            "Open frame after synthesised accept must seed the snapshot",
        );

        // A real modify must now fire Updated.
        let mut modified = order;
        modified.price = Decimal::from_str("2400.00").unwrap();
        dispatch_lighter_order(
            &modified,
            &rig.dispatch,
            &rig.emitter,
            &rig.registry,
            account_id(),
            trader_id(),
            UnixNanos::from(3),
        );
        let events = drain_events(&mut rig.rx);
        let updated = events
            .iter()
            .find(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Updated(_))));
        assert!(
            updated.is_some(),
            "real modify must produce Updated, events={events:?}"
        );
    }

    fn enqueue_create(client: &LighterExecutionClient, order: &OrderAny, nonce: i64) -> i64 {
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        client
            .dispatch
            .register_cloid(client_order_index, order.client_order_id());
        client.dispatch.register_order_identity(
            order.client_order_id(),
            OrderIdentity {
                instrument_id: order.instrument_id(),
                strategy_id: order.strategy_id(),
                order_side: order.order_side(),
                order_type: order.order_type(),
            },
        );
        let now = UnixNanos::from(1_000_000_000);
        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Create {
                order: Box::new(order.clone()),
                client_order_index,
            },
            submitted_at: now,
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });
        client.dispatch.nonce_manager.refresh(
            TEST_ACCOUNT_INDEX_I64,
            TEST_API_KEY_INDEX,
            nonce + 1,
        );
        let _ = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX);
        client_order_index
    }

    fn enqueue_other(client: &LighterExecutionClient, nonce: i64) {
        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Other,
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });
    }

    fn enqueue_cancel(
        client: &LighterExecutionClient,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        nonce: i64,
    ) {
        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Cancel {
                strategy_id: strategy_id(),
                instrument_id,
                client_order_id,
                venue_order_id: Some(venue_order_id),
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });
    }

    fn enqueue_modify(
        client: &LighterExecutionClient,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        nonce: i64,
    ) {
        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Modify {
                strategy_id: strategy_id(),
                instrument_id,
                client_order_id,
                venue_order_id: Some(venue_order_id),
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });
    }

    #[tokio::test]
    async fn handle_send_tx_ack_removes_hash_matched_entry() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order_a = test_limit_order(&mut factory, instrument_id, "ACK-A");
        let order_b = test_limit_order(&mut factory, instrument_id, "ACK-B");
        enqueue_create(&client, &order_a, 10);
        enqueue_create(&client, &order_b, 11);

        // Out-of-order ack: B's hash must remove B even though A is at head.
        let acked = handle_send_tx_ack(
            &client.dispatch,
            Some(TEST_ACCOUNT_INDEX_I64),
            200,
            Some("hash0b"),
        );

        assert!(matches!(
            acked.map(|pending| pending.kind),
            Some(PendingSendTxKind::Create { .. }),
        ));
        assert_eq!(client.dispatch.pending_sendtx_len(), 1, "only B pops");
        let head = client.dispatch.pop_pending_sendtx_head().unwrap();
        match head.kind {
            PendingSendTxKind::Create { order, .. } => {
                assert_eq!(order.client_order_id(), order_a.client_order_id());
            }
            _ => panic!("expected Create kind"),
        }
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "ack must not emit an event",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_ack_unmatched_hash_pops_nothing() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "ACK-UNMATCHED");
        enqueue_create(&client, &order, 10);

        let acked = handle_send_tx_ack(
            &client.dispatch,
            Some(TEST_ACCOUNT_INDEX_I64),
            200,
            Some("0xabc"),
        );

        assert!(acked.is_none());
        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            1,
            "an echoed hash with no matching entry must not pop the head",
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "unmatched ack must not emit an event",
        );
    }

    #[tokio::test]
    async fn prepared_create_tx_hash_round_trips_through_ack() {
        // The hash prepare threads into the queue must be the lowercase hex
        // form a venue ack echoes, or every live ack goes unattributed.
        let (client, cache, _rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "ACK-REAL-HASH");

        let credential = test_credential();
        let prepared = client
            .prepare_signed_create_order(&order, &credential, 0)
            .expect("prepare must sign");

        assert_eq!(prepared.tx_hash.len(), TX_HASH_BYTES * 2);
        assert!(
            prepared
                .tx_hash
                .chars()
                .all(|c| matches!(c, '0'..='9' | 'a'..='f')),
            "tx_hash must be lowercase hex, was `{}`",
            prepared.tx_hash,
        );

        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Create {
                order: Box::new(order),
                client_order_index: prepared.client_order_index,
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce: prepared.nonce,
            api_key_index: prepared.api_key_index,
            tx_hash: prepared.tx_hash.clone(),
        });

        let acked = handle_send_tx_ack(
            &client.dispatch,
            Some(TEST_ACCOUNT_INDEX_I64),
            200,
            Some(&prepared.tx_hash),
        );

        assert!(acked.is_some());
        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            0,
            "venue echo of the signed hash must match the enqueued entry",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_ack_returns_cancel_for_noop_probe() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("ACK-CANCEL-NOOP");
        let venue_order_id = VenueOrderId::from("123");
        enqueue_cancel(&client, instrument_id, client_order_id, venue_order_id, 12);

        let acked = handle_send_tx_ack(
            &client.dispatch,
            Some(TEST_ACCOUNT_INDEX_I64),
            200,
            Some("hash0c"),
        )
        .expect("acked cancel pending entry");
        let probe = AckedOrderProbe::from_pending(&acked).expect("cancel should schedule probe");

        match probe {
            AckedOrderProbe::Cancel {
                strategy_id: actual_strategy_id,
                instrument_id: actual_instrument_id,
                client_order_id: actual_client_order_id,
                venue_order_id: actual_venue_order_id,
            } => {
                assert_eq!(actual_strategy_id, strategy_id());
                assert_eq!(actual_instrument_id, instrument_id);
                assert_eq!(actual_client_order_id, client_order_id);
                assert_eq!(actual_venue_order_id, Some(venue_order_id));
            }
            other => panic!("expected cancel probe, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "ack itself must not emit before the no-op probe runs",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_ack_returns_modify_for_noop_probe() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("ACK-MODIFY-NOOP");
        let venue_order_id = VenueOrderId::from("456");
        enqueue_modify(&client, instrument_id, client_order_id, venue_order_id, 13);

        let acked = handle_send_tx_ack(
            &client.dispatch,
            Some(TEST_ACCOUNT_INDEX_I64),
            200,
            Some("hash0d"),
        )
        .expect("acked modify pending entry");
        let probe = AckedOrderProbe::from_pending(&acked).expect("modify should schedule probe");

        match probe {
            AckedOrderProbe::Modify {
                strategy_id: actual_strategy_id,
                instrument_id: actual_instrument_id,
                client_order_id: actual_client_order_id,
                venue_order_id: actual_venue_order_id,
            } => {
                assert_eq!(actual_strategy_id, strategy_id());
                assert_eq!(actual_instrument_id, instrument_id);
                assert_eq!(actual_client_order_id, client_order_id);
                assert_eq!(actual_venue_order_id, Some(venue_order_id));
            }
            other => panic!("expected modify probe, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "ack itself must not emit before the no-op probe runs",
        );
    }

    #[tokio::test]
    async fn ack_noop_probe_missing_cancel_emits_cancel_rejected() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("ACK-CANCEL-MISSING");
        let venue_order_id = VenueOrderId::from("123");
        let probe = AckedOrderProbe::Cancel {
            strategy_id: strategy_id(),
            instrument_id,
            client_order_id,
            venue_order_id: Some(venue_order_id),
        };

        assert!(emit_ack_noop_rejection_if_missing(
            &probe,
            false,
            &client.emitter,
            UnixNanos::from(1),
        ));

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::CancelRejected(e) => {
                assert_eq!(e.client_order_id, client_order_id);
                assert_eq!(e.instrument_id, instrument_id);
                assert_eq!(e.venue_order_id, Some(venue_order_id));
                assert!(e.reason.as_str().contains("order not found"));
            }
            other => panic!("expected CancelRejected, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn ack_noop_probe_missing_modify_emits_modify_rejected() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("ACK-MODIFY-MISSING");
        let venue_order_id = VenueOrderId::from("456");
        let probe = AckedOrderProbe::Modify {
            strategy_id: strategy_id(),
            instrument_id,
            client_order_id,
            venue_order_id: Some(venue_order_id),
        };

        assert!(emit_ack_noop_rejection_if_missing(
            &probe,
            false,
            &client.emitter,
            UnixNanos::from(1),
        ));

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::ModifyRejected(e) => {
                assert_eq!(e.client_order_id, client_order_id);
                assert_eq!(e.instrument_id, instrument_id);
                assert_eq!(e.venue_order_id, Some(venue_order_id));
                assert!(e.reason.as_str().contains("order not found"));
            }
            other => panic!("expected ModifyRejected, was {other:?}"),
        }
    }

    #[tokio::test]
    async fn ack_noop_probe_found_order_skips_rejection() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let probe = AckedOrderProbe::Cancel {
            strategy_id: strategy_id(),
            instrument_id,
            client_order_id: ClientOrderId::from("ACK-CANCEL-FOUND"),
            venue_order_id: Some(VenueOrderId::from("123")),
        };

        assert!(!emit_ack_noop_rejection_if_missing(
            &probe,
            true,
            &client.emitter,
            UnixNanos::from(1),
        ));
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "found acked order must not emit a no-op rejection",
        );
    }

    #[tokio::test]
    async fn send_tx_acks_recover_skip_window_past_window_size() {
        let (client, _cache, _rx) = create_execution_client();
        let window = i64::from(client.dispatch.nonce_manager.skip_window());

        for _ in 0..window {
            let nonce = client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
            enqueue_other(&client, nonce);
        }
        assert!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .is_err(),
            "window must trip after {window} unacked txs",
        );

        // Each venue ack reopens one slot, carrying issuance past 2x the
        // window without a refresh.
        for i in 0..window {
            let acked =
                handle_send_tx_ack(&client.dispatch, Some(TEST_ACCOUNT_INDEX_I64), 200, None);
            assert!(matches!(
                acked.map(|pending| pending.kind),
                Some(PendingSendTxKind::Other),
            ));
            let nonce = client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
            assert_eq!(nonce, TEST_NEXT_NONCE + window + i);
            enqueue_other(&client, nonce);
        }
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_rolls_back_latest_nonce() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "REJECT-LATEST");

        let nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        client
            .dispatch
            .register_cloid(client_order_index, order.client_order_id());
        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Create {
                order: Box::new(order.clone()),
                client_order_index,
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "invalid price",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        assert!(matches!(event, OrderEventAny::Rejected(_)));
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_with_newer_issuance_skips_rollback() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "REJECT-STALE");

        let rejected_nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();
        let newer_nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();
        let client_order_index = client
            .dispatch
            .derive_client_order_index(&order.client_order_id());
        client
            .dispatch
            .register_cloid(client_order_index, order.client_order_id());
        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Create {
                order: Box::new(order.clone()),
                client_order_index,
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce: rejected_nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{rejected_nonce:02x}"),
        });

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "invalid price",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        assert!(matches!(event, OrderEventAny::Rejected(_)));
        // The newer nonce is signed into an in-flight tx: the failed nonce
        // must not be freed for reissue.
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .last_issued(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(newer_nonce),
        );
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            newer_nonce + 1,
        );
    }

    async fn spawn_next_nonce_server(nonce: i64) -> String {
        let body = serde_json::to_string(&LighterNextNonce {
            code: 200,
            message: None,
            nonce,
        })
        .unwrap();
        let app = Router::new().route(
            "/api/v1/nextNonce",
            get(move || {
                let body = body.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        format!("http://{addr}")
    }

    #[tokio::test]
    async fn skip_window_exhaustion_resyncs_baseline_from_venue() {
        let venue_next_nonce = 100;
        let mut config = test_config();
        config.base_url_http = Some(spawn_next_nonce_server(venue_next_nonce).await);
        let (client, _cache, _rx) = create_execution_client_with_config(config);
        let credential = test_credential();

        let window = i64::from(client.dispatch.nonce_manager.skip_window());
        for _ in 0..window {
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
        }

        let err = client
            .build_tx_context(&credential)
            .expect_err("window must trip");
        assert!(
            err.to_string().contains("skip-window exhausted"),
            "unexpected error, was {err}",
        );

        wait_for_spawned_tasks(&client).await;

        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .baseline(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(venue_next_nonce - 1),
            "venue resync must advance the baseline",
        );
        assert!(
            !client.nonce_recovery_inflight.load(Ordering::Acquire),
            "recovery latch must release for the next exhaustion",
        );
        let context = client.build_tx_context(&credential).unwrap().context;
        assert_eq!(
            context.nonce, venue_next_nonce,
            "allocation must resume at the venue nonce",
        );
    }

    #[tokio::test]
    async fn skip_window_recovery_fetch_failure_releases_latch() {
        // 404 fails the fetch fast; unroutable addresses retry past the task wait
        let app = Router::new();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let mut config = test_config();
        config.base_url_http = Some(format!("http://{addr}"));
        let (client, _cache, _rx) = create_execution_client_with_config(config);
        let credential = test_credential();

        let window = i64::from(client.dispatch.nonce_manager.skip_window());
        for _ in 0..window {
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
        }
        client
            .build_tx_context(&credential)
            .expect_err("window must trip");

        wait_for_spawned_tasks(&client).await;

        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .baseline(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(TEST_NEXT_NONCE - 1),
            "failed venue fetch must leave the baseline unchanged",
        );
        assert!(
            !client.nonce_recovery_inflight.load(Ordering::Acquire),
            "failed venue fetch must release the recovery latch",
        );
    }

    #[tokio::test]
    async fn skip_window_recovery_dedupes_concurrent_fetches() {
        let hits = Arc::new(AtomicUsize::new(0));
        let server_hits = Arc::clone(&hits);
        let body = serde_json::to_string(&LighterNextNonce {
            code: 200,
            message: None,
            nonce: 100,
        })
        .unwrap();
        let app = Router::new().route(
            "/api/v1/nextNonce",
            get(move || {
                server_hits.fetch_add(1, Ordering::AcqRel);
                let body = body.clone();
                async move { body }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        let mut config = test_config();
        config.base_url_http = Some(format!("http://{addr}"));
        let (client, _cache, _rx) = create_execution_client_with_config(config);
        let credential = test_credential();

        let window = i64::from(client.dispatch.nonce_manager.skip_window());
        for _ in 0..window {
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap();
        }

        // Back-to-back exhaustions land inside one HTTP round trip; the
        // latch must collapse them into a single fetch.
        client
            .build_tx_context(&credential)
            .expect_err("window must trip");
        client
            .build_tx_context(&credential)
            .expect_err("window must still be exhausted");

        wait_for_spawned_tasks(&client).await;

        assert_eq!(
            hits.load(Ordering::Acquire),
            1,
            "burst exhaustion must trigger a single venue fetch",
        );
        assert!(!client.nonce_recovery_inflight.load(Ordering::Acquire));
    }

    #[tokio::test]
    async fn refresh_nonce_releases_recovery_latch() {
        let venue_next_nonce = 77;
        let mut config = test_config();
        config.base_url_http = Some(spawn_next_nonce_server(venue_next_nonce).await);
        let (client, _cache, _rx) = create_execution_client_with_config(config);

        // Simulate a recovery task aborted between latch set and clear
        client
            .nonce_recovery_inflight
            .store(true, Ordering::Release);

        client.refresh_nonce().await.unwrap();

        assert!(
            !client.nonce_recovery_inflight.load(Ordering::Acquire),
            "connect-time refresh must release a stuck recovery latch",
        );
        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
                .unwrap(),
            venue_next_nonce,
            "refresh must hard-reset allocation to the venue nonce",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_ack_create_emits_order_rejected() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "REJECT-CREATE");
        let client_order_index = enqueue_create(&client, &order, 42);

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "invalid price",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, order.client_order_id());
                assert!(e.reason.as_str().contains("code=21702"));
                assert!(e.reason.as_str().contains("invalid price"));
                assert!(!e.due_post_only);
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(client.dispatch.cloid_map.get(&client_order_index).is_none());
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_ack_create_sets_due_post_only() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "REJECT-POST-ONLY");
        enqueue_create(&client, &order, 43);

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21700),
            "post-only order would execute",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, order.client_order_id());
                assert!(e.due_post_only);
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_cancel_emits_cancel_rejected() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("REJECT-CANCEL");
        let venue_order_id = VenueOrderId::from("123");
        let nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();

        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Cancel {
                strategy_id: strategy_id(),
                instrument_id,
                client_order_id,
                venue_order_id: Some(venue_order_id),
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21727),
            "order is not cancelable",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::CancelRejected(e) => {
                assert_eq!(e.client_order_id, client_order_id);
                assert_eq!(e.instrument_id, instrument_id);
                assert_eq!(e.venue_order_id, Some(venue_order_id));
                assert!(e.reason.as_str().contains("code=21727"));
                assert!(e.reason.as_str().contains("order is not cancelable"));
            }
            other => panic!("expected CancelRejected, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_modify_emits_modify_rejected() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let client_order_id = ClientOrderId::from("REJECT-MODIFY");
        let venue_order_id = VenueOrderId::from("456");
        let nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();

        client.dispatch.enqueue_pending_sendtx(PendingSendTx {
            kind: PendingSendTxKind::Modify {
                strategy_id: strategy_id(),
                instrument_id,
                client_order_id,
                venue_order_id: Some(venue_order_id),
            },
            submitted_at: UnixNanos::from(1_000_000_000),
            nonce,
            api_key_index: TEST_API_KEY_INDEX,
            tx_hash: format!("hash{nonce:02x}"),
        });

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "modify rejected by venue",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::ModifyRejected(e) => {
                assert_eq!(e.client_order_id, client_order_id);
                assert_eq!(e.instrument_id, instrument_id);
                assert_eq!(e.venue_order_id, Some(venue_order_id));
                assert!(e.reason.as_str().contains("code=21702"));
                assert!(e.reason.as_str().contains("modify rejected by venue"));
            }
            other => panic!("expected ModifyRejected, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_hash_match_attributes_past_the_head() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order_a = test_limit_order(&mut factory, instrument_id, "REJECT-HASH-A");
        let order_b = test_limit_order(&mut factory, instrument_id, "REJECT-HASH-B");
        enqueue_create(&client, &order_a, 10);
        enqueue_create(&client, &order_b, 11);

        // A desynced or out-of-order rejection for B must not consume A.
        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "invalid price",
            Some("hash0b"),
        );

        let event = recv_order_event(&mut rx).await;
        match event {
            OrderEventAny::Rejected(e) => {
                assert_eq!(e.client_order_id, order_b.client_order_id());
            }
            other => panic!("expected Rejected, was {other:?}"),
        }
        assert_eq!(client.dispatch.pending_sendtx_len(), 1, "A must survive");
        let head = client.dispatch.pop_pending_sendtx_head().unwrap();
        match head.kind {
            PendingSendTxKind::Create { order, .. } => {
                assert_eq!(order.client_order_id(), order_a.client_order_id());
            }
            _ => panic!("expected Create kind"),
        }
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_unmatched_hash_pops_nothing() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "REJECT-UNMATCHED");
        enqueue_create(&client, &order, 10);

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "invalid price",
            Some("0xbeef"),
        );

        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            1,
            "an echoed hash with no matching entry must not pop the head",
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "unmatched rejection must not emit an event",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_bare_error_within_window_attributes() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "BARE-IN");
        enqueue_create(&client, &order, 50);

        let within_window = UnixNanos::from(1_000_000_000 + 500 * 1_000_000);
        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            within_window,
            SendTxRejectionSource::BareError,
            Some(21149),
            "integrator is not approved",
            None,
        );

        let event = recv_order_event(&mut rx).await;
        assert!(matches!(event, OrderEventAny::Rejected(_)));
        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_bare_error_outside_window_skips() {
        let (client, cache, mut rx) = create_execution_client();
        let instrument_id = register_test_instrument(&client, &cache);
        let mut factory = test_order_factory();
        let order = test_limit_order(&mut factory, instrument_id, "BARE-OUT");
        enqueue_create(&client, &order, 60);

        let outside_window = UnixNanos::from(1_000_000_000 + 2_000 * 1_000_000);
        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            outside_window,
            SendTxRejectionSource::BareError,
            Some(99),
            "late error",
            None,
        );

        assert_eq!(
            client.dispatch.pending_sendtx_len(),
            1,
            "head must remain queued past the 1s attribution window",
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "no event must be emitted outside the window",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_other_kind_rolls_back_latest_nonce() {
        let (client, _cache, _rx) = create_execution_client();
        let nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();
        enqueue_other(&client, nonce);

        let needs_resync = handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(23000),
            "Too Many Requests",
            None,
        );

        assert!(!needs_resync, "rate-limit rejection must not force resync");
        assert_nonce_reusable(&client.dispatch);
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_other_kind_skips_rollback_with_newer_issuance() {
        let (client, _cache, _rx) = create_execution_client();
        let rejected_nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();
        let newer_nonce = client
            .dispatch
            .nonce_manager
            .next_nonce(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX)
            .unwrap();
        enqueue_other(&client, rejected_nonce);

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(23000),
            "Too Many Requests",
            None,
        );

        assert_eq!(
            client
                .dispatch
                .nonce_manager
                .last_issued(TEST_ACCOUNT_INDEX_I64, TEST_API_KEY_INDEX),
            Some(newer_nonce),
            "non-latest rejection must leave last_issued alone",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_invalid_nonce_signals_resync() {
        let (client, _cache, _rx) = create_execution_client();

        enqueue_other(&client, 70);
        let attributed = handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(LIGHTER_ERROR_CODE_INVALID_NONCE),
            "invalid nonce",
            None,
        );
        assert!(attributed, "attributed invalid nonce must signal resync");

        let unattributed = handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(LIGHTER_ERROR_CODE_INVALID_NONCE),
            "invalid nonce",
            None,
        );
        assert!(
            unattributed,
            "unattributed invalid nonce must still signal resync",
        );

        enqueue_other(&client, 71);
        let other_code = handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21702),
            "invalid price",
            None,
        );
        assert!(!other_code, "other rejection codes must not force resync");
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_other_kind_logs_and_skips_emit() {
        let (client, _cache, mut rx) = create_execution_client();
        enqueue_other(&client, 70);

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(21727),
            "invalid client order index",
            None,
        );

        assert_eq!(client.dispatch.pending_sendtx_len(), 0, "Other head pops");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
            "Other-kind rejection must not emit OrderRejected",
        );
    }

    #[tokio::test]
    async fn handle_send_tx_rejection_empty_queue_logs_warn() {
        let (client, _cache, mut rx) = create_execution_client();

        handle_send_tx_rejection(
            &client.dispatch,
            &client.emitter,
            Some(TEST_ACCOUNT_INDEX_I64),
            UnixNanos::from(1_000_000_000),
            SendTxRejectionSource::Ack,
            Some(1),
            "no pending",
            None,
        );

        assert_eq!(client.dispatch.pending_sendtx_len(), 0);
        assert!(
            tokio::time::timeout(Duration::from_millis(50), rx.recv())
                .await
                .is_err(),
        );
    }

    #[rstest]
    #[case::standard_no_override(LighterAccountTier::Standard, None, 60, None)]
    #[case::standard_zero_is_default(LighterAccountTier::Standard, Some(0), 60, None)]
    #[case::standard_override_above_tier(
        LighterAccountTier::Standard,
        Some(24_000),
        24_000,
        Some(TierCrossCheck::AboveTier { documented: 60 })
    )]
    #[case::premium_raise_hint(
        LighterAccountTier::Premium,
        None,
        60,
        Some(TierCrossCheck::RaiseHint { documented: 24_000 })
    )]
    #[case::premium_configured_no_advisory(LighterAccountTier::Premium, Some(24_000), 24_000, None)]
    #[case::unknown_no_advisory(LighterAccountTier::Unknown(7), None, 60, None)]
    fn test_tier_quota_report(
        #[case] tier: LighterAccountTier,
        #[case] configured: Option<u32>,
        #[case] expected_active: u32,
        #[case] expected_cross_check: Option<TierCrossCheck>,
    ) {
        assert_eq!(
            tier_quota_report(tier, configured, 60),
            (expected_active, expected_cross_check),
        );
    }
}
