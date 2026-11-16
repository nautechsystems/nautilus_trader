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

//! Process-isolated tick-to-trade benchmarks through the real live engines.

#![cfg(not(madsim))]

use std::{any::Any, cell::RefCell, rc::Rc, time::Instant};

use async_trait::async_trait;
use nautilus_common::{
    cache::CacheView,
    clients::ExecutionClient,
    clock::Clock,
    factories::{ClientConfig, ExecutionClientFactory},
    live::runner::{get_data_event_sender, get_exec_event_sender},
    messages::{DataEvent, execution::SubmitOrder},
};
use nautilus_core::{Params, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{ExecutionEventEmitter, builder::LiveNodeBuilder};
use nautilus_model::{
    accounts::{AccountAny, MarginAccount},
    data::{Data, QuoteTick, TradeTick},
    enums::{AccountType, AggressorSide, OmsType},
    events::AccountState,
    identifiers::{AccountId, ClientId, InstrumentId, TradeId, TraderId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny, stubs::crypto_perpetual_ethusdt},
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity},
};
use rstest::rstest;

#[path = "../tests/common/tick_to_trade.rs"]
mod workload;

use workload::{Case, DATA_CLIENT, EXEC_CLIENT, Observation};

const ACCOUNT_ID: &str = "BENCH-001";

#[rstest]
#[case::quote_checked(true, false)]
#[case::quote_bypass(true, true)]
#[case::trade_checked(false, false)]
#[case::trade_bypass(false, true)]
#[ignore = "Wall-clock benchmark; run optimized with process isolation"]
#[tokio::test(flavor = "current_thread")]
async fn tick_to_trade_engine(
    #[case] quote: bool,
    #[case] bypass: bool,
    #[values(false, true)] stream: bool,
) {
    Box::pin(run(Case::new(quote, bypass, false, stream))).await;
}

#[cfg(feature = "python")]
#[rstest]
#[case::quote_checked(true, false)]
#[case::quote_bypass(true, true)]
#[case::trade_checked(false, false)]
#[case::trade_bypass(false, true)]
#[ignore = "Wall-clock benchmark; run optimized with process isolation"]
#[tokio::test(flavor = "current_thread")]
async fn tick_to_trade_engine_python(
    #[case] quote: bool,
    #[case] bypass: bool,
    #[values(false, true)] stream: bool,
) {
    Box::pin(run(Case::new(quote, bypass, true, stream))).await;
}

async fn run(case: Case) {
    let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt());
    let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
    let mut config = workload::node_config(case);
    config.data_engine.external_clients = Some(vec![ClientId::from(DATA_CLIENT)]);

    let node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .add_exec_client(
            Some(EXEC_CLIENT.to_string()),
            Box::new(SinkFactory {
                sender: sender.clone(),
                venue: instrument.id().venue,
            }),
            Box::new(SinkConfig),
        )
        .unwrap()
        .build()
        .unwrap();

    node.kernel()
        .cache()
        .borrow_mut()
        .add_instrument(instrument.clone())
        .unwrap();
    let account_id = AccountId::from(ACCOUNT_ID);
    let balance = Money::from("1000000 USDT");

    let account = MarginAccount::new(
        AccountState::new(
            account_id,
            AccountType::Margin,
            vec![AccountBalance::new(balance, Money::from("0 USDT"), balance)],
            vec![],
            true,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(Currency::USDT()),
        ),
        true,
    );

    node.kernel()
        .cache()
        .borrow_mut()
        .add_account(AccountAny::Margin(account))
        .unwrap();
    let data_sender = get_data_event_sender();
    let instrument_id = instrument.id();
    workload::measure(
        node,
        instrument,
        case,
        "engine",
        include_str!("tick_to_trade.rs"),
        receiver,
        move |ack| {
            workload::produce(case, ack, move |index, scheduled| {
                let data = tick(instrument_id, case.quote, index);
                let sent = Instant::now();
                data_sender.send(DataEvent::Data(data)).unwrap();
                sender
                    .send(Observation::Sent {
                        index,
                        scheduled,
                        sent,
                    })
                    .unwrap();
            })
        },
    )
    .await;
}

#[derive(Debug)]
struct SinkConfig;

impl ClientConfig for SinkConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[derive(Debug)]
struct SinkFactory {
    sender: tokio::sync::mpsc::UnboundedSender<Observation>,
    venue: Venue,
}

impl ExecutionClientFactory for SinkFactory {
    fn create(
        &self,
        trader_id: TraderId,
        _name: &str,
        _config: &dyn ClientConfig,
        cache: CacheView,
        _clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn ExecutionClient>> {
        Ok(Box::new(Sink {
            connected: false,
            cache,
            sender: self.sender.clone(),
            venue: self.venue,
            emitter: ExecutionEventEmitter::new(
                get_atomic_clock_realtime(),
                trader_id,
                AccountId::from(ACCOUNT_ID),
                AccountType::Margin,
                Some(Currency::USDT()),
            ),
        }))
    }

    fn name(&self) -> &'static str {
        "tick-to-trade-sink"
    }

    fn config_type(&self) -> &'static str {
        "SinkConfig"
    }
}

struct Sink {
    connected: bool,
    cache: CacheView,
    sender: tokio::sync::mpsc::UnboundedSender<Observation>,
    venue: Venue,
    emitter: ExecutionEventEmitter,
}

#[async_trait(?Send)]
impl ExecutionClient for Sink {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn client_id(&self) -> ClientId {
        ClientId::from(EXEC_CLIENT)
    }

    fn account_id(&self) -> AccountId {
        AccountId::from(ACCOUNT_ID)
    }

    fn venue(&self) -> Venue {
        self.venue
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.cache
            .borrow()
            .account(&self.account_id())
            .map(|account| account.clone())
    }

    fn generate_account_state(
        &self,
        _balances: Vec<AccountBalance>,
        _margins: Vec<MarginBalance>,
        _reported: bool,
        _ts_event: UnixNanos,
        _info: Option<Params>,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.emitter.set_sender(get_exec_event_sender());
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.connected = true;
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.connected = false;
        Ok(())
    }

    fn submit_order(&self, command: SubmitOrder) -> anyhow::Result<()> {
        let received = Instant::now();
        let order = self
            .cache
            .borrow()
            .order_owned(&command.client_order_id)
            .unwrap();
        let index = workload::order_index(command.client_order_id.as_str());
        self.sender
            .send(Observation::Received {
                index,
                received,
                order: workload::order_snapshot(&order),
            })
            .unwrap();

        self.emitter.emit_order_submitted(&order);
        let venue_id = VenueOrderId::from(format!("V-TTT-{index}"));
        self.emitter
            .emit_order_accepted(&order, venue_id, command.ts_init);
        self.emitter
            .emit_order_canceled(&order, Some(venue_id), command.ts_init);
        Ok(())
    }
}

fn tick(instrument_id: InstrumentId, quote: bool, index: usize) -> Data {
    let ts = UnixNanos::from(index as u64 + 1);

    if quote {
        Data::Quote(QuoteTick {
            instrument_id,
            bid_price: workload::price(index),
            ask_price: workload::price_ask(index),
            bid_size: Quantity::from(workload::SIZE_BID),
            ask_size: Quantity::from(workload::SIZE_ASK),
            ts_event: ts,
            ts_init: ts,
        })
    } else {
        Data::Trade(TradeTick {
            instrument_id,
            price: workload::price(index),
            size: Quantity::from(workload::SIZE_BID),
            aggressor_side: AggressorSide::Buy,
            trade_id: TradeId::from(format!("T-TTT-{index}")),
            ts_event: ts,
            ts_init: ts,
        })
    }
}
