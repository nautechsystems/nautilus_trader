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

use std::{cell::RefCell, path::PathBuf, rc::Rc, time::Duration};

use nautilus_common::{
    actor::{DataActor, DataActorCore},
    cache::CacheView,
    clients::DataClient,
    clock::Clock,
    enums::Environment,
    factories::{ClientConfig, DataClientFactory},
    messages::data::{
        DataCommand, SubscribeBars, SubscribeCommand, SubscribeQuotes, SubscribeTrades,
        UnsubscribeBars, UnsubscribeCommand, UnsubscribeQuotes, UnsubscribeTrades,
    },
    nautilus_actor,
};
use nautilus_core::{Params, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_databento::{data::DatabentoDataClientConfig, factories::DatabentoDataClientFactory};
use nautilus_live::node::LiveNode;
use nautilus_model::{
    data::{Bar, BarType, Data, QuoteTick, TradeTick},
    enums::AggressorSide,
    identifiers::{ClientId, InstrumentId, TradeId, TraderId, Venue, futures::FuturesYearDigits},
    types::{Price, Quantity},
};
use rstest::rstest;
use serde_json::json;

use crate::common::{
    TEST_DATASET, TEST_KEY, mbp1_msg, mock_server::MockLsgServer, ohlcv_msg, symbol_mapping_msg,
    trade_msg,
};

#[derive(Debug)]
struct RecordingFactory {
    commands: Rc<RefCell<Vec<DataCommand>>>,
}

impl DataClientFactory for RecordingFactory {
    fn create(
        &self,
        name: &str,
        config: &dyn ClientConfig,
        cache: CacheView,
        clock: Rc<RefCell<dyn Clock>>,
    ) -> anyhow::Result<Box<dyn DataClient>> {
        let client = DatabentoDataClientFactory::new().create(name, config, cache, clock)?;
        Ok(Box::new(RecordingClient {
            client,
            commands: Rc::clone(&self.commands),
        }))
    }

    fn name(&self) -> &'static str {
        "DATABENTO"
    }

    fn config_type(&self) -> &'static str {
        "DatabentoDataClientConfig"
    }
}

struct RecordingClient {
    client: Box<dyn DataClient>,
    commands: Rc<RefCell<Vec<DataCommand>>>,
}

#[async_trait::async_trait(?Send)]
impl DataClient for RecordingClient {
    fn client_id(&self) -> ClientId {
        self.client.client_id()
    }

    fn venue(&self) -> Option<Venue> {
        self.client.venue()
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.client.start()
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.client.stop()
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.client.reset()
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.client.dispose()
    }

    fn is_connected(&self) -> bool {
        self.client.is_connected()
    }

    fn is_disconnected(&self) -> bool {
        self.client.is_disconnected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        self.client.connect().await
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.client.disconnect().await
    }

    fn subscribe_quotes(&mut self, command: SubscribeQuotes) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(DataCommand::Subscribe(SubscribeCommand::Quotes(
                command.clone(),
            )));
        self.client.subscribe_quotes(command)
    }

    fn unsubscribe_quotes(&mut self, command: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(DataCommand::Unsubscribe(UnsubscribeCommand::Quotes(
                command.clone(),
            )));
        self.client.unsubscribe_quotes(command)
    }

    fn subscribe_trades(&mut self, command: SubscribeTrades) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(DataCommand::Subscribe(SubscribeCommand::Trades(
                command.clone(),
            )));
        self.client.subscribe_trades(command)
    }

    fn unsubscribe_trades(&mut self, command: &UnsubscribeTrades) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(DataCommand::Unsubscribe(UnsubscribeCommand::Trades(
                command.clone(),
            )));
        self.client.unsubscribe_trades(command)
    }

    fn subscribe_bars(&mut self, command: SubscribeBars) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(DataCommand::Subscribe(SubscribeCommand::Bars(
                command.clone(),
            )));
        self.client.subscribe_bars(command)
    }

    fn unsubscribe_bars(&mut self, command: &UnsubscribeBars) -> anyhow::Result<()> {
        self.commands
            .borrow_mut()
            .push(DataCommand::Unsubscribe(UnsubscribeCommand::Bars(
                command.clone(),
            )));
        self.client.unsubscribe_bars(command)
    }
}

#[derive(Debug)]
struct BatchActor {
    core: DataActorCore,
    ids: [InstrumentId; 2],
    bars: [BarType; 2],
    data: tokio::sync::mpsc::UnboundedSender<Data>,
}

impl DataActor for BatchActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let params: Params =
            serde_json::from_value(json!({"price_precision": 2, "start_ns": 17})).unwrap();
        let client_id = Some(ClientId::from("DATABENTO"));
        self.subscribe_quotes(self.ids, client_id, Some(params.clone()));
        self.subscribe_trades(self.ids, client_id, Some(params.clone()));
        self.subscribe_bars(self.bars, client_id, Some(params));
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        self.unsubscribe_quotes(self.ids, None, None);
        self.unsubscribe_trades(self.ids, None, None);
        self.unsubscribe_bars(self.bars, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        self.data.send(Data::Quote(*quote))?;
        Ok(())
    }

    fn on_trade(&mut self, trade: &TradeTick) -> anyhow::Result<()> {
        self.data.send(Data::Trade(*trade))?;
        Ok(())
    }

    fn on_bar(&mut self, bar: &Bar) -> anyhow::Result<()> {
        self.data.send(Data::Bar(*bar))?;
        Ok(())
    }
}
nautilus_actor!(BatchActor);

#[rstest]
#[tokio::test]
async fn test_actor_lists_reach_databento_hooks_and_deliver_decoded_data() {
    let server = MockLsgServer::new(TEST_DATASET).await;
    server.authenticate();
    server.expect_subscription();
    server.start();
    for _ in 0..3 {
        server.expect_subscription();
    }

    server.send_record(symbol_mapping_msg(1, "ESM4"));
    server.send_record(symbol_mapping_msg(2, "NQM4"));
    server.send_record(mbp1_msg(1, 100_000_000_000, 101_000_000_000, b'A'));
    server.send_record(mbp1_msg(2, 200_000_000_000, 202_000_000_000, b'A'));
    server.send_record(trade_msg(1, 103_000_000_000, 7));
    server.send_record(trade_msg(2, 205_000_000_000, 11));
    server.send_record(ohlcv_msg(1));
    let mut minute = ohlcv_msg(1);
    minute.hd.rtype = databento::dbn::enums::rtype::OHLCV_1M;
    minute.hd.ts_event = 60_000_000_000;
    server.send_record(minute);

    let mut config = DatabentoDataClientConfig::new(
        TEST_KEY,
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("publishers.json"),
        false,
        false,
    );
    config.futures_year_digits = FuturesYearDigits::One;
    config.live_gateway_addr = Some(server.addr());
    let commands = Rc::new(RefCell::new(Vec::new()));

    let factory = RecordingFactory {
        commands: Rc::clone(&commands),
    };

    let mut node = LiveNode::builder(TraderId::from("TESTER-001"), Environment::Live)
        .unwrap()
        .with_name("DATABENTO-BATCH-TEST")
        .with_load_state(false)
        .with_save_state(false)
        .with_delay_post_stop_secs(0)
        .add_data_client(None, Box::new(factory), Box::new(config))
        .unwrap()
        .build()
        .unwrap();
    let ids = [
        InstrumentId::from("ESM4.GLBX"),
        InstrumentId::from("NQM4.GLBX"),
    ];
    let bars = [
        BarType::from("ESM4.GLBX-1-SECOND-LAST-EXTERNAL"),
        BarType::from("ESM4.GLBX-1-MINUTE-LAST-EXTERNAL"),
    ];
    let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
    node.add_actor(BatchActor {
        core: DataActorCore::new(Default::default()),
        ids,
        bars,
        data: sender,
    })
    .unwrap();

    let handle = node.handle();
    let started = get_atomic_clock_realtime().get_time_ns();
    let mut received = Vec::new();
    {
        let run = node.run();
        tokio::pin!(run);

        let collect = async {
            for _ in 0..6 {
                received.push(receiver.recv().await.expect("actor data channel closed"));
            }
        };

        tokio::select! {
            result = &mut run => panic!("node stopped before data arrived: {result:?}"),
            result = tokio::time::timeout(Duration::from_secs(15), collect) => result.expect("data did not reach the actor"),
        }
        handle.stop();
        tokio::time::timeout(Duration::from_secs(10), run)
            .await
            .unwrap()
            .unwrap();
    }

    let ended = get_atomic_clock_realtime().get_time_ns();
    server.stop().await;
    assert!(!node.is_running());
    assert_hook_lists(&commands.borrow(), &ids, &bars, started, ended);
    assert_eq!(received.len(), 6);

    for (index, data) in received.iter().enumerate() {
        match data {
            Data::Quote(quote) => {
                let (id, bid, ask) = if index == 0 {
                    (ids[0], "100.00", "101.00")
                } else {
                    assert_eq!(index, 1);
                    (ids[1], "200.00", "202.00")
                };

                assert!(quote.ts_init >= started && quote.ts_init <= ended);
                assert_eq!(
                    *quote,
                    QuoteTick::new(
                        id,
                        Price::from(bid),
                        Price::from(ask),
                        Quantity::from(100),
                        Quantity::from(50),
                        UnixNanos::from(1_000_000_000),
                        quote.ts_init
                    )
                );
            }
            Data::Trade(trade) => {
                let (id, price, size) = if index == 2 {
                    (ids[0], "103.00", 7)
                } else {
                    assert_eq!(index, 3);
                    (ids[1], "205.00", 11)
                };

                assert_eq!(
                    *trade,
                    TradeTick::new(
                        id,
                        Price::from(price),
                        Quantity::from(size),
                        AggressorSide::Sell,
                        TradeId::from("1"),
                        UnixNanos::from(1_000_000_000),
                        trade.ts_init
                    )
                );
                assert!(trade.ts_init >= started && trade.ts_init <= ended);
            }
            Data::Bar(bar) => {
                assert!(index == 4 || index == 5);
                assert!(bar.ts_init >= started && bar.ts_init <= ended);
                assert_eq!(
                    *bar,
                    Bar::new(
                        bars[index - 4],
                        Price::from("100.00"),
                        Price::from("102.00"),
                        Price::from("99.00"),
                        Price::from("101.00"),
                        Quantity::from(1000),
                        UnixNanos::from(if index == 4 {
                            1_000_000_000
                        } else {
                            60_000_000_000
                        }),
                        bar.ts_init
                    )
                );
            }
            other => panic!("unexpected actor data: {other:?}"),
        }
    }
}

fn assert_hook_lists(
    recorded: &[DataCommand],
    ids: &[InstrumentId],
    bars: &[BarType],
    started: UnixNanos,
    ended: UnixNanos,
) {
    assert_eq!(recorded.len(), 6);

    for (index, (subscribe, release)) in recorded[..3].iter().zip(&recorded[3..]).enumerate() {
        let (DataCommand::Subscribe(subscribe), DataCommand::Unsubscribe(release)) =
            (subscribe, release)
        else {
            panic!("expected subscribe and unsubscribe hooks");
        };

        assert_eq!(subscribe.client_id(), Some(&ClientId::from("DATABENTO")));
        assert_eq!(subscribe.venue(), Some(&Venue::from("GLBX")));
        assert_eq!(subscribe.correlation_id(), None);
        assert!(subscribe.ts_init() >= started && subscribe.ts_init() <= ended);
        assert!(release.ts_init() >= subscribe.ts_init() && release.ts_init() <= ended);
        assert_ne!(subscribe.command_id(), release.command_id());
        assert_eq!(
            subscribe.params(),
            Some(
                &serde_json::from_value::<Params>(json!({"price_precision": 2, "start_ns": 17}))
                    .unwrap()
            )
        );

        if index < 2 {
            assert_eq!(subscribe.instrument_ids(), Some(ids));
        } else {
            assert_eq!(subscribe.bar_types(), Some(bars));
        }

        let expected =
            subscribe
                .clone()
                .into_unsubscribe(release.command_id(), release.ts_init(), None);
        assert_eq!(
            serde_json::to_value(release).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
    }
}
