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

//! Explicitly authorized live validation of the wallet-wide cancel-on-disconnect setting.

use std::{cell::RefCell, rc::Rc, time::Duration};

use anyhow::Context;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::replace_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{ModifyOrder, SubmitOrder},
    },
};
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_derive::{
    common::{
        consts::DERIVE_VENUE,
        credential::DeriveCredential,
        enums::{DeriveEnvironment, DeriveOrderCancelReason, DeriveOrderStatus},
        parse::parse_derive_instrument_any,
        urls,
    },
    config::DeriveExecutionClientConfig,
    execution::DeriveExecutionClient,
    http::{
        DeriveCredentials, DeriveHttpClient,
        error::DeriveHttpError,
        models::DeriveOrder,
        parse::parse_derive_subaccount_to_balances,
        query::{
            DeriveCancelByLabelParams, DeriveGetOrderParams, DeriveGetSubaccountParams,
            DeriveGetTriggerOrdersParams,
        },
    },
    websocket::{
        DeriveOrdersSubscriptionData, DeriveWebSocketClient, DeriveWsChannel, DeriveWsCredentials,
        DeriveWsMessage,
    },
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{AccountState as NautilusAccountState, OrderEventAny},
    identifiers::{AccountId, ClientId, ClientOrderId, StrategyId, TraderId},
    instruments::Instrument,
    orders::OrderTestBuilder,
    types::{Price, Quantity},
};
use nautilus_network::websocket::TransportBackend;
use rstest::rstest;
use rust_decimal_macros::dec;
use serde::Deserialize;
use serde_json::json;

#[derive(Deserialize)]
struct AccountState {
    cancel_on_disconnect: bool,
    subaccount_ids: Vec<u64>,
}

#[rstest]
#[case::testnet(DeriveEnvironment::Testnet)]
#[case::mainnet(DeriveEnvironment::Mainnet)]
#[tokio::test]
#[ignore = "mutates wallet-wide COD; requires explicit authorization, credentials, and an idle wallet"]
async fn test_live_cancel_on_disconnect(#[case] environment: DeriveEnvironment) {
    let credential = DeriveCredential::resolve(None, None, None, environment).unwrap();
    let credentials =
        DeriveCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(environment),
        credentials,
        Some(10),
        None,
        None,
    )
    .unwrap();
    let account = idle_account(&http, credential.wallet_address())
        .await
        .unwrap();
    assert!(
        !account.cancel_on_disconnect,
        "COD must be disabled before this validation"
    );
    let credentials =
        DeriveWsCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let mut ws_client = DeriveWebSocketClient::with_credentials(
        None,
        environment,
        TransportBackend::default(),
        None,
        credentials,
        None,
        None,
    );
    ws_client.set_request_timeout(Duration::from_secs(10));
    ws_client.connect().await.expect("live login failed");
    let validation: anyhow::Result<()> = async {
        ws_client
            .set_cancel_on_disconnect(true)
            .await
            .map_err(|e| anyhow::anyhow!("enable COD failed: {e}"))?;
        wait_for_cancel_on_disconnect(&http, credential.wallet_address(), true).await?;
        ws_client
            .disconnect()
            .await
            .map_err(|_| anyhow::anyhow!("disconnect failed"))?;
        ws_client
            .connect()
            .await
            .map_err(|_| anyhow::anyhow!("reconnect failed"))?;
        wait_for_cancel_on_disconnect(&http, credential.wallet_address(), true).await?;
        Ok(())
    }
    .await;

    if !ws_client.is_authenticated() {
        ws_client.connect().await.expect("cleanup login failed");
    }

    let cleanup = ws_client.set_cancel_on_disconnect(false).await;
    let disconnected = ws_client.disconnect().await;
    let final_account = wait_for_cancel_on_disconnect(&http, credential.wallet_address(), false)
        .await
        .expect("final idle-wallet check failed");
    assert!(
        !final_account.cancel_on_disconnect,
        "COD cleanup did not persist"
    );
    assert_eq!(final_account.subaccount_ids, account.subaccount_ids);
    cleanup.expect("disable COD failed");
    disconnected.expect("cleanup disconnect failed");
    validation.unwrap();
    println!(
        "{environment:?}: enable, reconnect persistence, disable, and idle-wallet checks passed"
    );
}

#[tokio::test]
#[ignore = "places and replaces a testnet order; requires authorization, credentials, and an idle wallet"]
async fn test_live_execution_cancel_on_disconnect() {
    let environment = DeriveEnvironment::Testnet;
    let credential = DeriveCredential::resolve(None, None, None, environment).unwrap();
    let credentials =
        DeriveCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let http = DeriveHttpClient::with_credentials(
        urls::rest_url(environment),
        credentials,
        Some(10),
        None,
        None,
    )
    .unwrap();
    let initial = idle_account(&http, credential.wallet_address())
        .await
        .unwrap();
    assert!(
        !initial.cancel_on_disconnect,
        "COD must be disabled before validation"
    );
    let instrument = http.get_instrument("ETH-PERP").await.unwrap();
    let clock = get_atomic_clock_realtime();
    let native = parse_derive_instrument_any(&instrument, clock.get_time_ns())
        .unwrap()
        .unwrap();
    let ticker = http.get_ticker("ETH-PERP").await.unwrap();
    let amount =
        (instrument.minimum_amount / instrument.amount_step).ceil() * instrument.amount_step;
    let quantity = Quantity::from_decimal_dp(amount, native.size_precision()).unwrap();
    let limit = (ticker.best_bid_price * dec!(0.95)).max(ticker.min_price + instrument.tick_size);
    let limit = (limit / instrument.tick_size).floor() * instrument.tick_size;
    let price = Price::from_decimal_dp(limit, native.price_precision()).unwrap();
    assert!(
        limit < ticker.best_bid_price && limit > ticker.min_price,
        "no passive price within venue bands"
    );
    let account_id = AccountId::from("DERIVE-001");
    let trader_id = TraderId::from("TESTER-001");
    let client_id = ClientId::from("DERIVE");
    let strategy_id = StrategyId::from("COD-TEST");
    let client_order_id = ClientOrderId::from(format!("COD-{}", UUID4::new()));
    let instrument_id = native.id();
    let cache = Rc::new(RefCell::new(Cache::default()));
    let snapshot = http
        .get_subaccount(&DeriveGetSubaccountParams::new(credential.subaccount_id()))
        .await
        .unwrap();
    let (balances, margins, info) = parse_derive_subaccount_to_balances(&snapshot).unwrap();
    let account = NautilusAccountState::new(
        account_id,
        AccountType::Margin,
        balances,
        margins,
        true,
        UUID4::new(),
        clock.get_time_ns(),
        clock.get_time_ns(),
        None,
    )
    .with_info(Some(info));
    cache
        .borrow_mut()
        .add_account(AccountAny::Margin(MarginAccount::new(account, true)))
        .unwrap();
    cache.borrow_mut().add_instrument(native).unwrap();

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *DERIVE_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Margin,
        None,
        cache.clone(),
    );
    let config = DeriveExecutionClientConfig::builder()
        .environment(environment)
        .wallet_address(credential.wallet_address().to_string())
        .session_key(credential.session_key().into())
        .subaccount_id(credential.subaccount_id())
        .max_fee_per_contract(dec!(1000))
        .signature_expiry_secs(900)
        .cancel_on_disconnect(true)
        .build();
    let (tx, mut events) = tokio::sync::mpsc::unbounded_channel();
    replace_exec_event_sender(tx);
    let mut client = DeriveExecutionClient::new(core, config).unwrap();
    client.start().unwrap();
    let observer_credentials =
        DeriveWsCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let mut observer = DeriveWebSocketClient::with_credentials(
        None,
        environment,
        TransportBackend::default(),
        None,
        observer_credentials,
        None,
        None,
    );
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(trader_id)
        .strategy_id(strategy_id)
        .instrument_id(instrument_id)
        .client_order_id(client_order_id)
        .side(OrderSide::Buy)
        .quantity(quantity)
        .price(price)
        .time_in_force(TimeInForce::Gtc)
        .post_only(true)
        .build();
    cache
        .borrow_mut()
        .add_order(order.clone(), None, None, false)
        .unwrap();

    let validation: anyhow::Result<()> = async {
        client.connect().await.context("execution connect failed")?;
        observer
            .connect()
            .await
            .context("observer connect failed")?;
        observer
            .subscribe_channels(vec![DeriveWsChannel::orders(credential.subaccount_id())])
            .await?;
        client.submit_order(SubmitOrder::from_order(
            &order,
            trader_id,
            Some(client_id),
            None,
            UUID4::new(),
            UnixNanos::default(),
        ))?;
        let accepted = live_order_event(&mut events, client_order_id).await?;

        let OrderEventAny::Accepted(accepted) = accepted else {
            anyhow::bail!("live submit was not accepted");
        };

        let posted = http
            .get_order(&DeriveGetOrderParams::new(
                credential.subaccount_id(),
                accepted.venue_order_id.as_str(),
            ))
            .await?;
        anyhow::ensure!(
            posted.mmp
                && posted.filled_amount == dec!(0)
                && posted.amount == amount
                && posted.limit_price == limit,
            "live order fields or MMP mismatch"
        );
        client.modify_order(ModifyOrder::new(
            trader_id,
            Some(client_id),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(accepted.venue_order_id),
            None,
            Some(Price::from_decimal_dp(
                limit - instrument.tick_size,
                price.precision,
            )?),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))?;
        let updated = live_order_event(&mut events, client_order_id).await?;

        let OrderEventAny::Updated(updated) = updated else {
            anyhow::bail!("live replace was not accepted");
        };

        let venue_order_id = updated
            .venue_order_id
            .context("replacement venue ID missing")?;
        let replaced = http
            .get_order(&DeriveGetOrderParams::new(
                credential.subaccount_id(),
                venue_order_id.as_str(),
            ))
            .await?;
        anyhow::ensure!(
            replaced.mmp
                && replaced.filled_amount == dec!(0)
                && replaced.amount == amount
                && replaced.limit_price == limit - instrument.tick_size,
            "live replacement fields or MMP mismatch"
        );
        client.disconnect().await?;

        let cancelled = tokio::time::timeout(Duration::from_secs(30), async {
            while let Some(event) = observer.next_event().await {
                if let DeriveWsMessage::Subscription(payload) = event {
                    let data: DeriveOrdersSubscriptionData =
                        serde_json::from_str(payload.data.get())?;

                    for order in data.orders {
                        if order.order_id == venue_order_id.as_str()
                            && order.order_status == DeriveOrderStatus::Cancelled
                        {
                            return Ok::<DeriveOrder, anyhow::Error>(order);
                        }
                    }
                }
            }

            anyhow::bail!("COD observer stream closed")
        })
        .await
        .context("COD order cancellation timed out")??;

        anyhow::ensure!(
            cancelled.cancel_reason == DeriveOrderCancelReason::CancelOnDisconnect
                && cancelled.mmp
                && cancelled.filled_amount == dec!(0)
                && cancelled.amount == amount
                && cancelled.limit_price == limit - instrument.tick_size,
            "order did not cancel through COD without fills"
        );
        Ok(())
    }
    .await;

    let disconnected = client.disconnect().await;
    let observer_disconnected = observer.disconnect().await;
    let cancelled = http
        .cancel_by_label(&DeriveCancelByLabelParams::new(
            credential.subaccount_id(),
            client_order_id.as_str(),
        ))
        .await;
    let credentials =
        DeriveWsCredentials::new(credential.wallet_address(), credential.session_key()).unwrap();
    let mut admin = DeriveWebSocketClient::with_credentials(
        None,
        environment,
        TransportBackend::default(),
        None,
        credentials,
        None,
        None,
    );
    admin.connect().await.expect("cleanup login failed");
    let disabled = admin.set_cancel_on_disconnect(false).await;
    let admin_disconnected = admin.disconnect().await;
    let final_account = wait_for_cancel_on_disconnect(&http, credential.wallet_address(), false)
        .await
        .expect("final idle-wallet check failed");
    assert_eq!(final_account.subaccount_ids, initial.subaccount_ids);
    disconnected.unwrap();

    match cancelled {
        Ok(_) | Err(DeriveHttpError::JsonRpc { code: 11006, .. }) => {}
        Err(e) => panic!("label cleanup failed: {e}"),
    }

    disabled.unwrap();
    admin_disconnected.unwrap();
    observer_disconnected.unwrap();
    validation.unwrap();
    println!(
        "Testnet execution config: protected submit, replacement, disconnect cancellation, and cleanup passed"
    );
}

async fn live_order_event(
    events: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    client_order_id: ClientOrderId,
) -> anyhow::Result<OrderEventAny> {
    tokio::time::timeout(Duration::from_secs(20), async {
        while let Some(event) = events.recv().await {
            if let ExecutionEvent::Order(event) = event
                && event.client_order_id() == client_order_id
                && matches!(
                    event,
                    OrderEventAny::Accepted(_)
                        | OrderEventAny::Updated(_)
                        | OrderEventAny::Rejected(_)
                        | OrderEventAny::Denied(_)
                        | OrderEventAny::ModifyRejected(_)
                        | OrderEventAny::Filled(_)
                )
            {
                return Ok(event);
            }
        }

        anyhow::bail!("execution event stream closed")
    })
    .await
    .context("live order acknowledgement timed out")?
}

async fn wait_for_cancel_on_disconnect(
    http: &DeriveHttpClient,
    wallet: &str,
    enabled: bool,
) -> anyhow::Result<AccountState> {
    tokio::time::timeout(Duration::from_secs(30), async {
        loop {
            let account = idle_account(http, wallet).await?;
            if account.cancel_on_disconnect == enabled {
                return Ok(account);
            }

            tokio::time::sleep(Duration::from_millis(250)).await;
        }
    })
    .await
    .map_err(|_| {
        anyhow::anyhow!("account did not reflect acknowledged COD setting within 30 seconds")
    })?
}

async fn idle_account(http: &DeriveHttpClient, wallet: &str) -> anyhow::Result<AccountState> {
    let account: AccountState = http
        .send_private("private/get_account", &json!({"wallet": wallet}))
        .await
        .map_err(|e| match e {
            DeriveHttpError::JsonRpc { code, .. } => {
                anyhow::anyhow!("account read failed: JSON-RPC {code}")
            }
            DeriveHttpError::Http { status, .. } => {
                anyhow::anyhow!("account read failed: HTTP {status}")
            }
            _ => anyhow::anyhow!("account read failed"),
        })?;

    anyhow::ensure!(
        !account.subaccount_ids.is_empty(),
        "wallet has no subaccounts"
    );

    for subaccount_id in &account.subaccount_ids {
        let params = DeriveGetSubaccountParams::new(*subaccount_id);
        let subaccount = http
            .get_subaccount(&params)
            .await
            .map_err(|_| anyhow::anyhow!("subaccount read failed"))?;
        anyhow::ensure!(subaccount.open_orders.is_empty(), "wallet has open orders");
        anyhow::ensure!(subaccount.positions.is_empty(), "wallet has positions");
        let params = DeriveGetTriggerOrdersParams::new(*subaccount_id);
        let orders = http
            .get_trigger_orders(&params)
            .await
            .map_err(|_| anyhow::anyhow!("trigger order read failed"))?;
        anyhow::ensure!(orders.orders.is_empty(), "wallet has trigger orders");
    }

    Ok(account)
}
