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

//! Shared execution client utilities for Binance Spot and Futures adapters.

use std::{
    future::Future,
    time::{Duration, Instant},
};

use nautilus_common::enums::LogLevel;
use nautilus_live::{ExecutionClientCore, task::TaskGroup};
use nautilus_model::{
    events::OrderEventAny,
    identifiers::AccountId,
    orders::{Order, OrderAny},
};

pub(crate) fn active_orders(core: &ExecutionClientCore) -> Vec<OrderAny> {
    let cache = core.cache();
    cache
        .orders_refs(Some(&core.venue), None, None, Some(&core.account_id), None)
        .into_iter()
        .filter(|order| {
            (order.is_open() || order.is_inflight())
                && !order.strategy_id().is_external()
                && cache.client_id(&order.client_order_id()) == Some(&core.client_id)
                && (order.ts_submitted().is_some()
                    || order.events().iter().any(|event| {
                        matches!(event, OrderEventAny::Accepted(accepted) if !accepted.reconciliation)
                    }))
        })
        .map(|order| order.cloned())
        .collect()
}

pub(crate) fn log_report_receipt(count: usize, report_type: &str, level: LogLevel) {
    let level = match level {
        LogLevel::Off => return,
        LogLevel::Trace => log::Level::Trace,
        LogLevel::Debug => log::Level::Debug,
        LogLevel::Info => log::Level::Info,
        LogLevel::Warning => log::Level::Warn,
        LogLevel::Error => log::Level::Error,
    };
    let plural = if count == 1 { "" } else { "s" };
    log::log!(level, "Received {count} {report_type}{plural}");
}

/// Spawns an async task and tracks its handle in `pending_tasks`.
pub fn spawn_task<F>(pending_tasks: &TaskGroup, description: &'static str, fut: F)
where
    F: Future<Output = anyhow::Result<()>> + Send + 'static,
{
    let future = async move {
        if let Err(e) = fut.await {
            log::warn!("{description} failed: {e}");
        }
    };

    if let Err(e) = pending_tasks.spawn(future) {
        log::warn!("Skipping Binance {description} after shutdown began: {e}");
    }
}

/// Aborts all pending tasks stored in `pending_tasks`.
pub fn abort_pending_tasks(pending_tasks: &TaskGroup) {
    pending_tasks.begin_shutdown();
}

/// Completes bounded shutdown for Binance command tasks.
///
/// # Errors
///
/// Returns an error if bounded task shutdown fails.
pub async fn await_pending_tasks(pending_tasks: &TaskGroup) -> anyhow::Result<()> {
    pending_tasks.begin_shutdown();
    pending_tasks
        .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
        .await
        .map_err(|e| anyhow::anyhow!("Failed to terminate Binance execution tasks: {e}"))?;
    Ok(())
}

/// Polls the cache until the account is registered or timeout is reached.
///
/// Each iteration borrows and drops the cache Ref to avoid holding the
/// RefCell borrow across await points, which would block mutable access
/// when the account state is registered by another task.
///
/// # Errors
///
/// Returns an error if the timeout is reached before the account is registered.
pub async fn await_account_registered(
    core: &ExecutionClientCore,
    account_id: AccountId,
    timeout_secs: f64,
) -> anyhow::Result<()> {
    if core.cache().account(&account_id).is_some() {
        log::info!("Account {account_id} registered");
        return Ok(());
    }

    let start = Instant::now();
    let timeout = Duration::from_secs_f64(timeout_secs);
    let interval = Duration::from_millis(10);

    loop {
        tokio::time::sleep(interval).await;

        if core.cache().account(&account_id).is_some() {
            log::info!("Account {account_id} registered");
            return Ok(());
        }

        if start.elapsed() >= timeout {
            anyhow::bail!(
                "Timeout waiting for account {account_id} to be registered after {timeout_secs}s"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::cache::Cache;
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        accounts::{AccountAny, CashAccount},
        enums::{AccountType, OmsType, OrderType},
        events::AccountState,
        identifiers::{AccountId, TraderId},
        orders::{OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{AccountBalance, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::consts::{BINANCE_CLIENT_ID, BINANCE_VENUE};

    #[rstest]
    #[case::accepted(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, true, false),
        false,
        1
    )]
    #[case::submitted(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, false, false),
        false,
        1
    )]
    #[case::other_account(
        "BINANCE-002",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, true, false),
        false,
        0
    )]
    #[case::other_client(
        "BINANCE-001",
        "OTHER",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, true, false),
        false,
        0
    )]
    #[case::unbound(
        "BINANCE-001",
        "",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, true, false),
        false,
        0
    )]
    #[case::external(
        "BINANCE-001",
        "BINANCE",
        "EXTERNAL",
        "BTCUSDT.BINANCE",
        (true, true, false),
        false,
        0
    )]
    #[case::other_venue(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.OTHER",
        (true, true, false),
        false,
        0
    )]
    #[case::initialized(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (false, false, false),
        false,
        0
    )]
    #[case::closed(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, true, true),
        false,
        0
    )]
    #[case::claimed_external(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (false, true, false),
        true,
        0
    )]
    #[case::reconciled_local(
        "BINANCE-001",
        "BINANCE",
        "OWNED-002",
        "BTCUSDT.BINANCE",
        (true, true, false),
        true,
        1
    )]
    fn test_active_orders_selects_only_retained_local_owners(
        #[case] account: &str,
        #[case] client: &str,
        #[case] strategy: &str,
        #[case] instrument: &str,
        #[case] lifecycle: (bool, bool, bool),
        #[case] reconciled: bool,
        #[case] expected_count: usize,
    ) {
        let (submitted, accepted, closed) = lifecycle;
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = create_test_core(cache.clone(), AccountId::from("BINANCE-001"));
        let order = OrderTestBuilder::new(OrderType::Limit)
            .strategy_id(strategy.into())
            .instrument_id(instrument.into())
            .quantity(Quantity::from("0.002"))
            .price(Price::from("45678.90"))
            .build();
        let cid = order.client_order_id();
        cache
            .borrow_mut()
            .add_order(
                order.clone(),
                None,
                (!client.is_empty()).then(|| client.into()),
                false,
            )
            .unwrap();

        if submitted {
            cache
                .borrow_mut()
                .update_order(&TestOrderEventStubs::submitted(&order, account.into()))
                .unwrap();
        }

        if accepted {
            let OrderEventAny::Accepted(mut event) =
                TestOrderEventStubs::accepted(&order, account.into(), "123".into())
            else {
                panic!("Expected acceptance stub")
            };

            event.reconciliation = reconciled;
            cache
                .borrow_mut()
                .update_order(&OrderEventAny::Accepted(event))
                .unwrap();
        }

        if closed {
            cache
                .borrow_mut()
                .update_order(&TestOrderEventStubs::canceled(
                    &order,
                    account.into(),
                    Some("123".into()),
                ))
                .unwrap();
        }

        let selected = active_orders(&core);
        assert_eq!(selected.len(), expected_count);

        if expected_count == 1 {
            assert_eq!(selected[0], cache.borrow().order_owned(&cid).unwrap());
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_spawn_task_unregisters_finished_task_before_shutdown() {
        let pending_tasks = TaskGroup::new();

        spawn_task(&pending_tasks, "test task", async { Ok(()) });
        tokio::time::timeout(Duration::from_secs(1), async {
            while !pending_tasks.all_finished() {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("task should finish");

        assert!(pending_tasks.is_empty());
        abort_pending_tasks(&pending_tasks);
        await_pending_tasks(&pending_tasks)
            .await
            .expect("task shutdown");
        assert!(pending_tasks.is_empty());
    }

    #[rstest]
    #[tokio::test]
    async fn test_abort_pending_tasks_aborts_running_tasks() {
        let (drop_tx, drop_rx) = tokio::sync::oneshot::channel();
        let guard = AbortDropSignal { tx: Some(drop_tx) };

        let pending_tasks = TaskGroup::new();
        pending_tasks
            .spawn(async move {
                let _guard = guard;
                tokio::time::sleep(Duration::from_secs(60)).await;
            })
            .expect("task spawn");

        abort_pending_tasks(&pending_tasks);
        await_pending_tasks(&pending_tasks)
            .await
            .expect("task shutdown");

        assert!(pending_tasks.is_empty());
        tokio::time::timeout(Duration::from_secs(1), drop_rx)
            .await
            .expect("Aborted task should drop its future")
            .expect("Drop signal should be sent");
    }

    #[rstest]
    #[tokio::test]
    async fn test_await_account_registered_returns_when_account_is_added() {
        let account_id = AccountId::from("BINANCE-001");
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = create_test_core(cache.clone(), account_id);

        let wait_fut = await_account_registered(&core, account_id, 0.5);
        let register_fut = async move {
            tokio::time::sleep(Duration::from_millis(20)).await;
            add_test_account_to_cache(&cache, account_id);
        };

        let (result, ()) = tokio::join!(wait_fut, register_fut);
        result.unwrap();
    }

    #[rstest]
    #[tokio::test]
    async fn test_await_account_registered_times_out() {
        let account_id = AccountId::from("BINANCE-001");
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = create_test_core(cache, account_id);

        let error = await_account_registered(&core, account_id, 0.02)
            .await
            .expect_err("Missing account should time out");

        assert!(error.to_string().contains("BINANCE-001"));
    }

    struct AbortDropSignal {
        tx: Option<tokio::sync::oneshot::Sender<()>>,
    }

    impl Drop for AbortDropSignal {
        fn drop(&mut self) {
            if let Some(tx) = self.tx.take() {
                let _ = tx.send(());
            }
        }
    }

    fn create_test_core(cache: Rc<RefCell<Cache>>, account_id: AccountId) -> ExecutionClientCore {
        ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            *BINANCE_CLIENT_ID,
            *BINANCE_VENUE,
            OmsType::Hedging,
            account_id,
            AccountType::Cash,
            None,
            cache,
        )
    }

    fn add_test_account_to_cache(cache: &Rc<RefCell<Cache>>, account_id: AccountId) {
        let state = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::from("1.0 BTC"),
                Money::from("0 BTC"),
                Money::from("1.0 BTC"),
            )],
            vec![],
            true,
            nautilus_core::UUID4::new(),
            nautilus_core::UnixNanos::default(),
            nautilus_core::UnixNanos::default(),
            None,
        );

        let account = AccountAny::Cash(CashAccount::new(state, true, false));
        cache.borrow_mut().add_account(account).unwrap();
    }
}
