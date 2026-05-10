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

//! Live execution client for the Bullet adapter.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use bullet_exchange_interface::{
    address::Address,
    decimals::PositiveDecimal,
    message::{AmendOrderArgs, CancelOrderArgs, NewOrderArgs, UserAction},
    types::{
        ClientOrderId as BulletClientOrderId, MarketId, OrderId, OrderType as BulletOrderType,
        Side,
    },
};
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::execution::{CancelAllOrders, CancelOrder, ModifyOrder, SubmitOrder},
};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType},
    identifiers::{AccountId, ClientId, VenueOrderId, Venue},
    orders::{Order, any::OrderAny},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::BULLET_VENUE,
        credential::BulletCredential,
        error::BulletError,
        models::SymbolPrecision,
    },
    config::BulletExecClientConfig,
    http::client::BulletHttpClient,
    signing::{chain_data::ChainData, tx_builder::sign_user_action},
};

/// Live execution client for the Bullet exchange.
#[derive(Debug)]
pub struct BulletExecutionClient {
    core: ExecutionClientCore,
    emitter: ExecutionEventEmitter,
    config: BulletExecClientConfig,
    clock: &'static AtomicTime,
    http: Option<Arc<BulletHttpClient>>,
    creds: Option<Arc<BulletCredential>>,
    chain: Option<Arc<Mutex<ChainData>>>,
    sym_map: Option<Arc<HashMap<String, SymbolPrecision>>>,
    main_addr: Option<String>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl BulletExecutionClient {
    /// Create a new [`BulletExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if initialisation fails.
    pub fn new(
        core: ExecutionClientCore,
        config: BulletExecClientConfig,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );
        Ok(Self {
            core,
            emitter,
            config,
            clock,
            http: None,
            creds: None,
            chain: None,
            sym_map: None,
            main_addr: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn http(&self) -> anyhow::Result<&Arc<BulletHttpClient>> {
        self.http.as_ref().ok_or_else(|| anyhow::anyhow!("not connected — call connect() first"))
    }

    fn creds(&self) -> anyhow::Result<&Arc<BulletCredential>> {
        self.creds.as_ref().ok_or_else(|| anyhow::anyhow!("credentials not loaded"))
    }

    fn chain(&self) -> anyhow::Result<Arc<Mutex<ChainData>>> {
        self.chain.clone().ok_or_else(|| anyhow::anyhow!("chain data not loaded"))
    }

    fn sym_map(&self) -> anyhow::Result<&HashMap<String, SymbolPrecision>> {
        self.sym_map
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("symbol map not loaded"))
    }

    fn main_addr(&self) -> anyhow::Result<&str> {
        self.main_addr.as_deref().ok_or_else(|| anyhow::anyhow!("account address not set"))
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                tracing::warn!("{description} failed: {e:?}");
            }
        });
        let mut tasks = self.pending_tasks.lock().expect("pending_tasks mutex poisoned");
        tasks.retain(|h| !h.is_finished());
        tasks.push(handle);
    }

    fn symbol_for_order(order: &OrderAny) -> anyhow::Result<String> {
        let instrument_id = order.instrument_id();
        let sym = instrument_id.symbol.as_str();
        sym.strip_suffix("-PERP")
            .map(str::to_string)
            .ok_or_else(|| {
                anyhow::anyhow!("Bullet only supports -PERP instruments, got: {sym}")
            })
    }

    fn build_new_order_args(
        order: &OrderAny,
        info: &SymbolPrecision,
        is_buy: bool,
    ) -> anyhow::Result<NewOrderArgs> {
        let price = match order.price() {
            Some(p) => {
                let dec = crate::common::parse::snap_price(
                    p.as_decimal(),
                    info.tick_size,
                    is_buy,
                );
                PositiveDecimal::try_from(dec)
                    .map_err(|e| anyhow::anyhow!("invalid price: {e}"))?
            }
            None => {
                // Market orders: use a very conservative price placeholder.
                // On Bullet, IOC orders are matched at best available price
                // regardless of the submitted limit price, but a price must be present.
                PositiveDecimal::try_from(rust_decimal::Decimal::ONE)
                    .map_err(|e| anyhow::anyhow!("price build error: {e}"))?
            }
        };

        let qty_dec =
            crate::common::parse::snap_qty(order.quantity().as_decimal(), info.step_size);
        let size = PositiveDecimal::try_from(qty_dec)
            .map_err(|e| anyhow::anyhow!("invalid quantity: {e}"))?;

        let side = match order.order_side() {
            OrderSide::Buy => Side::Bid,
            OrderSide::Sell => Side::Ask,
            other => anyhow::bail!("unsupported order side: {other:?}"),
        };

        let order_type = match order.order_type() {
            OrderType::Limit => BulletOrderType::Limit,
            OrderType::LimitIfTouched => BulletOrderType::Limit,
            OrderType::Market => BulletOrderType::ImmediateOrCancel,
            other => anyhow::bail!("unsupported order type for Bullet: {other:?}"),
        };

        // Parse client_order_id as u64 for Bullet
        let client_order_id = order
            .client_order_id()
            .as_str()
            .parse::<u64>()
            .ok()
            .map(BulletClientOrderId);

        Ok(NewOrderArgs {
            price,
            size,
            side,
            order_type,
            reduce_only: order.is_reduce_only(),
            client_order_id,
            pending_tpsl_pair: None,
        })
    }

    async fn sign_and_submit(
        action: UserAction<Address>,
        creds: Arc<BulletCredential>,
        chain: Arc<Mutex<ChainData>>,
        http: Arc<BulletHttpClient>,
    ) -> anyhow::Result<String> {
        for attempt in 0..2u8 {
            let tx_b64 = {
                let chain_guard =
                    chain.lock().map_err(|_| anyhow::anyhow!("chain mutex poisoned"))?;
                sign_user_action(action.clone(), &creds, &chain_guard, None)
                    .map_err(|e| anyhow::anyhow!("signing failed: {e}"))?
            };
            match http.submit_tx(tx_b64).await {
                Ok(resp) => return Ok(resp.id),
                Err(BulletError::TransactionOutdated) if attempt == 0 => {
                    tracing::warn!("TransactionOutdated: refreshing chain data and retrying");
                    let info =
                        http.exchange_info().await.map_err(|e| anyhow::anyhow!("{e}"))?;
                    let new_chain = ChainData::from_exchange_info(&info)
                        .map_err(|e| anyhow::anyhow!("chain refresh: {e}"))?;
                    let mut guard = chain
                        .lock()
                        .map_err(|_| anyhow::anyhow!("chain mutex poisoned"))?;
                    *guard = new_chain;
                }
                Err(e) => return Err(anyhow::anyhow!("{e}")),
            }
        }
        Err(anyhow::anyhow!("sign_and_submit: exceeded retry limit"))
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BulletExecutionClient {
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
        *BULLET_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter.emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        // Load credential
        let creds = BulletCredential::resolve(
            self.config.private_key.as_deref(),
            self.config.key_file.as_deref(),
        )
        .map_err(|e| anyhow::anyhow!("credential error: {e}"))?;

        // Determine main account address
        let main_addr = self
            .config
            .account_address
            .clone()
            .unwrap_or_else(|| creds.address());

        // Build HTTP client
        let http = BulletHttpClient::new(
            self.config.http_url(),
            self.config.http_timeout_secs,
            self.config.proxy_url.clone(),
        )
        .map_err(|e| anyhow::anyhow!("HTTP client error: {e}"))?;

        // Fetch exchange info for chain data and symbol map
        let info = http.exchange_info().await.map_err(|e| anyhow::anyhow!("{e}"))?;
        let chain_data = ChainData::from_exchange_info(&info)
            .map_err(|e| anyhow::anyhow!("chain data: {e}"))?;
        let sym_map: HashMap<String, SymbolPrecision> = info
            .symbols
            .iter()
            .map(|s| (s.symbol.clone(), SymbolPrecision::from_symbol_info(s)))
            .collect();

        self.creds = Some(Arc::new(creds));
        self.http = Some(Arc::new(http));
        self.chain = Some(Arc::new(Mutex::new(chain_data)));
        self.sym_map = Some(Arc::new(sym_map));
        self.main_addr = Some(main_addr);
        self.core.set_connected();

        tracing::info!(
            client_id = %self.core.client_id,
            account = %self.core.account_id,
            environment = ?self.config.environment,
            "Bullet execution client connected",
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.core.set_disconnected();
        tracing::info!("Bullet execution client disconnected");
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }
        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();
        tracing::info!(
            client_id = %self.core.client_id,
            account_id = %self.core.account_id,
            environment = ?self.config.environment,
            "Bullet execution client started",
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }
        // Abort pending HTTP tasks
        let mut tasks = self.pending_tasks.lock().expect("pending_tasks mutex poisoned");
        for h in tasks.drain(..) {
            h.abort();
        }
        drop(tasks);
        self.core.set_disconnected();
        self.core.set_stopped();
        tracing::info!("Bullet execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        if order.is_closed() {
            tracing::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        let bullet_symbol = match Self::symbol_for_order(&order) {
            Ok(s) => s,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };

        let sym_map = self.sym_map()?;
        let info = match sym_map.get(&bullet_symbol) {
            Some(i) => i.clone(),
            None => {
                let msg = format!("Unknown symbol '{bullet_symbol}' — not in exchangeInfo");
                self.emitter.emit_order_denied(&order, &msg);
                return Ok(());
            }
        };

        let is_buy = order.order_side() == OrderSide::Buy;
        let new_order_args = match Self::build_new_order_args(&order, &info, is_buy) {
            Ok(a) => a,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };

        let action = UserAction::<Address>::PlaceOrders {
            market_id: MarketId(info.market_id),
            orders: vec![new_order_args],
            replace: false,
            sub_account_index: None,
        };

        self.emitter.emit_order_submitted(&order);

        let http = self.http()?.clone();
        let creds = self.creds()?.clone();
        let chain = self.chain()?;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("submit_order", async move {
            match Self::sign_and_submit(action, creds, chain, http).await {
                Ok(tx_id) => {
                    let venue_order_id = VenueOrderId::from(tx_id.as_str());
                    let ts = clock.get_time_ns();
                    emitter.emit_order_accepted(&order, venue_order_id, ts);
                }
                Err(e) => {
                    let ts = clock.get_time_ns();
                    emitter.emit_order_rejected(&order, &e.to_string(), ts, false);
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        if order.is_closed() {
            tracing::warn!("Cannot cancel closed order {}", order.client_order_id());
            return Ok(());
        }

        let bullet_symbol = Self::symbol_for_order(&order)?;
        let sym_map = self.sym_map()?;
        let info = sym_map
            .get(&bullet_symbol)
            .ok_or_else(|| anyhow::anyhow!("Unknown symbol '{bullet_symbol}'"))?;

        // Prefer cancelling by venue order id (Bullet OrderId)
        let venue_order_id: Option<OrderId> = order
            .venue_order_id()
            .and_then(|v| v.as_str().parse::<u64>().ok())
            .map(OrderId);

        // Fall back to client_order_id parsed as u64
        let client_order_id: Option<BulletClientOrderId> = cmd
            .client_order_id
            .as_str()
            .parse::<u64>()
            .ok()
            .map(BulletClientOrderId);

        // Bullet requires exactly one identifier — prefer venue order_id if known
        let cancel_args = if venue_order_id.is_some() {
            CancelOrderArgs { order_id: venue_order_id, client_order_id: None }
        } else {
            CancelOrderArgs { order_id: None, client_order_id }
        };
        let action = UserAction::<Address>::CancelOrders {
            market_id: MarketId(info.market_id),
            orders: vec![cancel_args],
            sub_account_index: None,
        };

        let http = self.http()?.clone();
        let creds = self.creds()?.clone();
        let chain = self.chain()?;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            match Self::sign_and_submit(action, creds, chain, http).await {
                Ok(_) => {
                    let ts = clock.get_time_ns();
                    emitter.emit_order_canceled(&order, order.venue_order_id(), ts);
                }
                Err(e) => {
                    let ts = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected(
                        &order,
                        order.venue_order_id(),
                        &e.to_string(),
                        ts,
                    );
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        if order.is_closed() {
            tracing::warn!("Cannot modify closed order {}", order.client_order_id());
            return Ok(());
        }

        let bullet_symbol = Self::symbol_for_order(&order)?;
        let sym_map = self.sym_map()?;
        let info = sym_map
            .get(&bullet_symbol)
            .ok_or_else(|| anyhow::anyhow!("Unknown symbol '{bullet_symbol}'"))?
            .clone();

        let venue_order_id: Option<OrderId> = order
            .venue_order_id()
            .and_then(|v| v.as_str().parse::<u64>().ok())
            .map(OrderId);
        let client_order_id: Option<BulletClientOrderId> = cmd
            .client_order_id
            .as_str()
            .parse::<u64>()
            .ok()
            .map(BulletClientOrderId);

        // Bullet requires exactly one identifier — prefer venue order_id if known
        let cancel_args = if venue_order_id.is_some() {
            CancelOrderArgs { order_id: venue_order_id, client_order_id: None }
        } else {
            CancelOrderArgs { order_id: None, client_order_id }
        };

        // Build new order from the modify command's updated fields
        let is_buy = order.order_side() == OrderSide::Buy;
        let new_qty = cmd.quantity.unwrap_or_else(|| order.quantity());
        let new_price = cmd.price.or_else(|| order.price());

        let new_price_pd = match new_price {
            Some(p) => {
                let snapped =
                    crate::common::parse::snap_price(p.as_decimal(), info.tick_size, is_buy);
                PositiveDecimal::try_from(snapped)
                    .map_err(|e| anyhow::anyhow!("invalid price: {e}"))?
            }
            None => anyhow::bail!("modify_order requires a price for Bullet"),
        };

        let qty_snapped =
            crate::common::parse::snap_qty(new_qty.as_decimal(), info.step_size);
        let new_size_pd =
            PositiveDecimal::try_from(qty_snapped).map_err(|e| anyhow::anyhow!("invalid qty: {e}"))?;

        let side = match order.order_side() {
            OrderSide::Buy => Side::Bid,
            OrderSide::Sell => Side::Ask,
            other => anyhow::bail!("unsupported side: {other:?}"),
        };

        let order_type = match order.order_type() {
            OrderType::Limit | OrderType::LimitIfTouched => BulletOrderType::Limit,
            other => anyhow::bail!("modify not supported for order type: {other:?}"),
        };

        let new_client_order_id = cmd
            .client_order_id
            .as_str()
            .parse::<u64>()
            .ok()
            .map(BulletClientOrderId);

        let new_order_args = NewOrderArgs {
            price: new_price_pd,
            size: new_size_pd,
            side,
            order_type,
            reduce_only: order.is_reduce_only(),
            client_order_id: new_client_order_id,
            pending_tpsl_pair: None,
        };

        let action = UserAction::<Address>::AmendOrders {
            market_id: MarketId(info.market_id),
            orders: vec![AmendOrderArgs { cancel: cancel_args, place: new_order_args }],
            sub_account_index: None,
        };

        let http = self.http()?.clone();
        let creds = self.creds()?.clone();
        let chain = self.chain()?;
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let new_qty_for_event = cmd.quantity.unwrap_or_else(|| order.quantity());

        self.spawn_task("modify_order", async move {
            match Self::sign_and_submit(action, creds, chain, http).await {
                Ok(tx_id) => {
                    let venue_id = order
                        .venue_order_id()
                        .unwrap_or_else(|| VenueOrderId::from(tx_id.as_str()));
                    let ts = clock.get_time_ns();
                    emitter.emit_order_updated(
                        &order,
                        venue_id,
                        new_qty_for_event,
                        new_price,
                        None,
                        None,
                        ts,
                    );
                }
                Err(e) => {
                    let ts = clock.get_time_ns();
                    emitter.emit_order_modify_rejected(
                        &order,
                        order.venue_order_id(),
                        &e.to_string(),
                        ts,
                    );
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let sym_str = cmd.instrument_id.symbol.as_str();
        let action = if let Some(bullet_symbol) = sym_str.strip_suffix("-PERP") {
            let sym_map = self.sym_map()?;
            let info = sym_map
                .get(bullet_symbol)
                .ok_or_else(|| anyhow::anyhow!("Unknown symbol '{bullet_symbol}'"))?;
            UserAction::<Address>::CancelMarketOrders {
                market_id: MarketId(info.market_id),
                sub_account_index: None,
            }
        } else {
            UserAction::<Address>::CancelAllOrders { sub_account_index: None }
        };

        let http = self.http()?.clone();
        let creds = self.creds()?.clone();
        let chain = self.chain()?;

        self.spawn_task("cancel_all_orders", async move {
            Self::sign_and_submit(action, creds, chain, http).await?;
            Ok(())
        });

        Ok(())
    }
}
