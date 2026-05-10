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

//! Python bindings for `BulletOrderClient` — a signing + HTTP order client for the Python layer.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use bullet_exchange_interface::{
    address::Address,
    decimals::PositiveDecimal,
    message::{AmendOrderArgs, CancelOrderArgs, NewOrderArgs, UserAction},
    types::{
        ClientOrderId as BulletClientOrderId, MarketId, OrderId, OrderType as BulletOrderType,
        Side,
    },
};
use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;
use rust_decimal::Decimal;

use crate::{
    common::{
        credential::BulletCredential,
        error::BulletError,
        models::SymbolPrecision,
        parse::{snap_price, snap_qty},
    },
    http::client::BulletHttpClient,
    signing::{chain_data::ChainData, tx_builder::sign_user_action},
};

/// Inner state populated after [`BulletOrderClient::connect`] succeeds.
#[derive(Default)]
struct ConnectedState {
    http: Option<Arc<BulletHttpClient>>,
    creds: Option<Arc<BulletCredential>>,
    chain: Option<Arc<Mutex<ChainData>>>,
    sym_map: Option<Arc<HashMap<String, SymbolPrecision>>>,
    main_addr: Option<String>,
}

/// Python-facing order client that bundles credential loading, signing, and HTTP submission.
///
/// Construct with [`BulletOrderClient::py_new`] then `await client.connect()` before placing orders.
#[derive(Clone, Debug)]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.bullet")]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")]
pub struct BulletOrderClient {
    // Shared mutable state — populated on connect().
    state: Arc<Mutex<ConnectedState>>,
    // Construction parameters for building the HTTP client on connect.
    base_url: String,
    timeout_secs: u64,
    proxy_url: Option<String>,
    private_key: Option<String>,
    key_file: Option<String>,
    account_address: Option<String>,
}

impl std::fmt::Debug for ConnectedState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectedState")
            .field("connected", &self.http.is_some())
            .field("main_addr", &self.main_addr)
            .finish()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BulletOrderClient {
    /// Create a new [`BulletOrderClient`].
    ///
    /// Call `await client.connect()` before placing any orders.
    #[new]
    #[pyo3(signature = (
        base_url,
        timeout_secs = 60,
        proxy_url = None,
        private_key = None,
        key_file = None,
        account_address = None,
    ))]
    fn py_new(
        base_url: String,
        timeout_secs: u64,
        proxy_url: Option<String>,
        private_key: Option<String>,
        key_file: Option<String>,
        account_address: Option<String>,
    ) -> Self {
        Self {
            state: Arc::new(Mutex::new(ConnectedState::default())),
            base_url,
            timeout_secs,
            proxy_url,
            private_key,
            key_file,
            account_address,
        }
    }

    /// Fetch chain data and exchange info, load credentials.
    ///
    /// Must be awaited before calling any order methods.
    #[pyo3(name = "connect")]
    fn py_connect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let base_url = self.base_url.clone();
        let timeout_secs = self.timeout_secs;
        let proxy_url = self.proxy_url.clone();
        let private_key = self.private_key.clone();
        let key_file = self.key_file.clone();
        let account_address = self.account_address.clone();
        let state = self.state.clone();

        future_into_py(py, async move {
            let http = BulletHttpClient::new(&base_url, timeout_secs, proxy_url)
                .map_err(to_pyruntime_err)?;

            let creds =
                BulletCredential::resolve(private_key.as_deref(), key_file.as_deref())
                    .map_err(to_pyruntime_err)?;

            let main_addr = account_address.unwrap_or_else(|| creds.address());

            let info = http.exchange_info().await.map_err(to_pyruntime_err)?;
            let chain_data = ChainData::from_exchange_info(&info)
                .map_err(|e| to_pyruntime_err(e.to_string()))?;
            let sym_map: HashMap<String, SymbolPrecision> = info
                .symbols
                .iter()
                .map(|s| (s.symbol.clone(), SymbolPrecision::from_symbol_info(s)))
                .collect();

            let mut guard =
                state.lock().map_err(|_| to_pyruntime_err("state mutex poisoned"))?;
            guard.http = Some(Arc::new(http));
            guard.creds = Some(Arc::new(creds));
            guard.chain = Some(Arc::new(Mutex::new(chain_data)));
            guard.sym_map = Some(Arc::new(sym_map));
            guard.main_addr = Some(main_addr);

            Ok(())
        })
    }

    /// Place a limit or market order.
    ///
    /// Returns the transaction ID string on success.
    #[pyo3(name = "place_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_place_order<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        is_buy: bool,
        price: String,
        qty: String,
        is_limit: bool,
        client_order_id: Option<u64>,
        reduce_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (http, creds, chain, market_id, tick_size, step_size) =
            self.resolve_symbol(&symbol)?;

        let price_dec: Decimal = price.parse().map_err(to_pyruntime_err)?;
        let qty_dec: Decimal = qty.parse().map_err(to_pyruntime_err)?;

        let snapped_price = snap_price(price_dec, tick_size, is_buy);
        let snapped_qty = snap_qty(qty_dec, step_size);

        let price_pd = PositiveDecimal::try_from(snapped_price)
            .map_err(|e| to_pyruntime_err(e.to_string()))?;
        let size_pd = PositiveDecimal::try_from(snapped_qty)
            .map_err(|e| to_pyruntime_err(e.to_string()))?;

        let side = if is_buy { Side::Bid } else { Side::Ask };
        let order_type = if is_limit {
            BulletOrderType::Limit
        } else {
            BulletOrderType::ImmediateOrCancel
        };

        let new_order = NewOrderArgs {
            price: price_pd,
            size: size_pd,
            side,
            order_type,
            reduce_only,
            client_order_id: client_order_id.map(BulletClientOrderId),
            pending_tpsl_pair: None,
        };

        let action = UserAction::<Address>::PlaceOrders {
            market_id: MarketId(market_id),
            orders: vec![new_order],
            replace: false,
            sub_account_index: None,
        };

        future_into_py(py, async move {
            Self::sign_and_submit(action, creds, chain, http).await.map_err(to_pyruntime_err)
        })
    }

    /// Cancel an order by venue order id or client order id.
    ///
    /// Returns the transaction ID string on success.
    #[pyo3(name = "cancel_order")]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        venue_order_id: Option<u64>,
        client_order_id: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (http, creds, chain, market_id, _, _) = self.resolve_symbol(&symbol)?;

        // Bullet requires exactly one identifier — prefer venue order_id if known
        let cancel_args = if venue_order_id.is_some() {
            CancelOrderArgs { order_id: venue_order_id.map(OrderId), client_order_id: None }
        } else {
            CancelOrderArgs { order_id: None, client_order_id: client_order_id.map(BulletClientOrderId) }
        };

        let action = UserAction::<Address>::CancelOrders {
            market_id: MarketId(market_id),
            orders: vec![cancel_args],
            sub_account_index: None,
        };

        future_into_py(py, async move {
            Self::sign_and_submit(action, creds, chain, http).await.map_err(to_pyruntime_err)
        })
    }

    /// Cancel multiple orders on one market in a single transaction.
    ///
    /// `orders` is a list of `(venue_order_id, client_order_id)` pairs; pass `None` for the
    /// identifier you don't have.  Prefer `venue_order_id` when available.
    ///
    /// Returns the transaction ID string on success.
    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        orders: Vec<(Option<u64>, Option<u64>)>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (http, creds, chain, market_id, _, _) = self.resolve_symbol(&symbol)?;

        let cancel_args: Vec<CancelOrderArgs> = orders
            .into_iter()
            .map(|(vid, cid)| {
                if vid.is_some() {
                    CancelOrderArgs { order_id: vid.map(OrderId), client_order_id: None }
                } else {
                    CancelOrderArgs {
                        order_id: None,
                        client_order_id: cid.map(BulletClientOrderId),
                    }
                }
            })
            .collect();

        let action = UserAction::<Address>::CancelOrders {
            market_id: MarketId(market_id),
            orders: cancel_args,
            sub_account_index: None,
        };

        future_into_py(py, async move {
            Self::sign_and_submit(action, creds, chain, http).await.map_err(to_pyruntime_err)
        })
    }

    /// Atomically amend (cancel + re-place) an order.
    ///
    /// Returns the transaction ID string on success.
    #[pyo3(name = "amend_order")]
    #[allow(clippy::too_many_arguments)]
    fn py_amend_order<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        is_buy: bool,
        venue_order_id: Option<u64>,
        client_order_id: Option<u64>,
        new_price: String,
        new_qty: String,
        new_client_order_id: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (http, creds, chain, market_id, tick_size, step_size) =
            self.resolve_symbol(&symbol)?;

        let price_dec: Decimal = new_price.parse().map_err(to_pyruntime_err)?;
        let qty_dec: Decimal = new_qty.parse().map_err(to_pyruntime_err)?;

        let snapped_price = snap_price(price_dec, tick_size, is_buy);
        let snapped_qty = snap_qty(qty_dec, step_size);

        let price_pd = PositiveDecimal::try_from(snapped_price)
            .map_err(|e| to_pyruntime_err(e.to_string()))?;
        let size_pd = PositiveDecimal::try_from(snapped_qty)
            .map_err(|e| to_pyruntime_err(e.to_string()))?;

        let side = if is_buy { Side::Bid } else { Side::Ask };

        let cancel_args = if venue_order_id.is_some() {
            CancelOrderArgs { order_id: venue_order_id.map(OrderId), client_order_id: None }
        } else {
            CancelOrderArgs { order_id: None, client_order_id: client_order_id.map(BulletClientOrderId) }
        };
        let new_order = NewOrderArgs {
            price: price_pd,
            size: size_pd,
            side,
            order_type: BulletOrderType::Limit,
            reduce_only: false,
            client_order_id: new_client_order_id.map(BulletClientOrderId),
            pending_tpsl_pair: None,
        };

        let action = UserAction::<Address>::AmendOrders {
            market_id: MarketId(market_id),
            orders: vec![AmendOrderArgs { cancel: cancel_args, place: new_order }],
            sub_account_index: None,
        };

        future_into_py(py, async move {
            Self::sign_and_submit(action, creds, chain, http).await.map_err(to_pyruntime_err)
        })
    }

    /// Cancel all open orders on a specific market.
    #[pyo3(name = "cancel_market_orders")]
    fn py_cancel_market_orders<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let (http, creds, chain, market_id, _, _) = self.resolve_symbol(&symbol)?;

        let action = UserAction::<Address>::CancelMarketOrders {
            market_id: MarketId(market_id),
            sub_account_index: None,
        };

        future_into_py(py, async move {
            Self::sign_and_submit(action, creds, chain, http).await.map_err(to_pyruntime_err)
        })
    }

    /// Cancel all open orders across all markets.
    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let guard =
            self.state.lock().map_err(|_| to_pyruntime_err("state mutex poisoned"))?;
        let http = guard
            .http
            .clone()
            .ok_or_else(|| to_pyruntime_err("not connected — call connect() first"))?;
        let creds = guard
            .creds
            .clone()
            .ok_or_else(|| to_pyruntime_err("credentials not loaded"))?;
        let chain = guard
            .chain
            .clone()
            .ok_or_else(|| to_pyruntime_err("chain data not loaded"))?;
        drop(guard);

        let action = UserAction::<Address>::CancelAllOrders { sub_account_index: None };

        future_into_py(py, async move {
            Self::sign_and_submit(action, creds, chain, http).await.map_err(to_pyruntime_err)
        })
    }

    /// Return whether this client has been connected (chain data loaded).
    #[getter]
    fn is_connected(&self) -> bool {
        self.state
            .lock()
            .map(|g| g.chain.is_some())
            .unwrap_or(false)
    }

    /// Return the main account address.
    #[getter]
    fn account_address(&self) -> Option<String> {
        self.state.lock().ok().and_then(|g| g.main_addr.clone())
    }

    fn __repr__(&self) -> String {
        format!(
            "BulletOrderClient(base_url='{}', connected={})",
            self.base_url,
            self.is_connected()
        )
    }
}

impl BulletOrderClient {
    fn resolve_symbol(
        &self,
        symbol: &str,
    ) -> PyResult<(
        Arc<BulletHttpClient>,
        Arc<BulletCredential>,
        Arc<Mutex<ChainData>>,
        u16,
        Option<Decimal>,
        Option<Decimal>,
    )> {
        let guard =
            self.state.lock().map_err(|_| to_pyruntime_err("state mutex poisoned"))?;
        let http = guard
            .http
            .clone()
            .ok_or_else(|| to_pyruntime_err("not connected — call connect() first"))?;
        let creds = guard
            .creds
            .clone()
            .ok_or_else(|| to_pyruntime_err("credentials not loaded"))?;
        let chain = guard
            .chain
            .clone()
            .ok_or_else(|| to_pyruntime_err("chain data not loaded"))?;
        let sym_map = guard
            .sym_map
            .as_deref()
            .ok_or_else(|| to_pyruntime_err("symbol map not loaded — call connect() first"))?;
        let info = sym_map
            .get(symbol)
            .ok_or_else(|| to_pyruntime_err(format!("Unknown symbol '{symbol}' — not in exchangeInfo")))?;
        let result = (http, creds, chain, info.market_id, info.tick_size, info.step_size);
        Ok(result)
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
