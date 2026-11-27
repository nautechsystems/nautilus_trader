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
    cell::{Cell, RefCell},
    fmt::Debug,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use bytes::Bytes;
use nautilus_common::{
    enums::{Environment, SerializationEncoding},
    msgbus::{
        self, BusMessage, BusPayloadType, MessageBusBacking, MessageBusBackingFactory,
        MessageBusConfig, MessageBusExternalIngress, TypedHandler,
    },
    testing::wait_until_async,
};
use nautilus_core::UUID4;
use nautilus_live::{
    builder::LiveNodeBuilder,
    config::{LiveExecutionEngineConfig, LiveNodeConfig},
    node::{LiveNode, NodeState},
};
use nautilus_model::{data::QuoteTick, identifiers::TraderId};
use parking_lot::Mutex;
use rstest::rstest;

use crate::external_msgbus_fixtures::{CapturingExternalEgress, SharedClosed};

#[rstest]
fn test_builder_with_external_msgbus_egress_uses_configured_encoding() {
    let (external_egress, publications, closed) = CapturingExternalEgress::new();

    let msgbus_config = MessageBusConfig {
        encoding: SerializationEncoding::Json,
        ..Default::default()
    };

    let node = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
        .unwrap()
        .with_msgbus_config(msgbus_config)
        .with_external_msgbus_egress(Box::new(external_egress))
        .build()
        .expect("node builds with external message bus egress");
    let quote = QuoteTick::default();

    msgbus::publish_quote("data.quotes.TEST".into(), &quote);

    let publications = publications.borrow();
    assert_eq!(publications.len(), 1);
    assert_eq!(publications[0].topic.as_str(), "data.quotes.TEST");
    assert_eq!(
        serde_json::from_slice::<QuoteTick>(&publications[0].payload)
            .expect("JSON payload must decode as QuoteTick"),
        quote
    );
    drop(publications);

    msgbus::get_message_bus().borrow_mut().dispose();
    assert!(closed.get());
    drop(node);
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_builder_with_external_msgbus_factory_installs_egress_and_ingress() {
    let quote = QuoteTick::default();
    let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
    let publications = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(AtomicBool::new(false));
    let factory = CapturingBackingFactory::new(publications.clone(), closed.clone(), Some(rx));

    let msgbus_config = MessageBusConfig {
        external_streams: Some(vec!["stream".to_string()]),
        ..Default::default()
    };

    let config = LiveNodeConfig {
        environment: Environment::Sandbox,
        msgbus: Some(msgbus_config),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::from_millis(500),
        timeout_disconnection: Duration::from_millis(500),
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_external_msgbus_factory(Box::new(factory))
        .build()
        .expect("node builds with external message bus factory");

    msgbus::publish_quote("data.quotes.TEST".into(), &quote);

    {
        let publications = publications.lock();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic.as_str(), "data.quotes.TEST");
        assert_eq!(
            serde_json::from_slice::<QuoteTick>(&publications[0].payload)
                .expect("JSON payload must decode as QuoteTick"),
            quote
        );
    }

    let received = Rc::new(RefCell::new(Vec::<QuoteTick>::new()));
    let handle = node.handle();

    // Stopping from `drive` rather than here, so the node cannot finish `run` before the
    // republished quote is observed.
    let handler = TypedHandler::from({
        let received = received.clone();
        move |quote: &QuoteTick| {
            received.borrow_mut().push(*quote);
        }
    });

    msgbus::subscribe_quotes("data.quotes.*".into(), handler, None);
    msgbus::get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::QuoteTick);

    let payload =
        Bytes::from(serde_json::to_vec(&quote).expect("QuoteTick should serialize as JSON"));
    let message = BusMessage::with_str_topic(
        "data.quotes.TEST",
        BusPayloadType::QuoteTick,
        payload,
        SerializationEncoding::Json,
    );

    tokio::time::timeout(Duration::from_secs(30), async {
        let run = node.run();
        tokio::pin!(run);

        let drive = async {
            wait_until_async(|| async { handle.is_running() }, Duration::from_secs(10)).await;

            tx.send(message)
                .await
                .expect("external ingress receiver should be open");

            wait_until_async(
                || async { received.borrow().len() == 1 },
                Duration::from_secs(10),
            )
            .await;
            assert_eq!(*received.borrow(), vec![quote]);
            handle.stop();
        };

        tokio::select! {
            biased;

            () = drive => {}
            result = &mut run => {
                panic!("node stopped before factory ingress was republished: {result:?}");
            }
        }

        run.await.expect("node should stop cleanly");
    })
    .await
    .expect("live node should republish factory ingress and stop before timeout");

    assert_eq!(handle.state(), NodeState::Stopped);
    assert!(closed.load(Ordering::Relaxed));
    msgbus::get_message_bus().borrow_mut().dispose();
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_builder_with_external_msgbus_factory_without_streams_runs_without_ingress() {
    let quote = QuoteTick::default();
    let publications = Arc::new(Mutex::new(Vec::new()));
    let closed = Arc::new(AtomicBool::new(false));
    let factory = CapturingBackingFactory::new(publications.clone(), closed.clone(), None);

    let config = LiveNodeConfig {
        environment: Environment::Sandbox,
        msgbus: Some(MessageBusConfig::default()),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::from_millis(500),
        timeout_disconnection: Duration::from_millis(500),
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_external_msgbus_factory(Box::new(factory))
        .build()
        .expect("node builds with egress-only message bus factory");
    let handle = node.handle();

    msgbus::publish_quote("data.quotes.TEST".into(), &quote);

    {
        let publications = publications.lock();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic.as_str(), "data.quotes.TEST");
    }

    tokio::time::timeout(Duration::from_secs(30), async {
        let run = node.run();
        tokio::pin!(run);

        let drive = async {
            wait_until_async(|| async { handle.is_running() }, Duration::from_secs(10)).await;
            handle.stop();
        };

        tokio::select! {
            biased;

            () = drive => {}
            result = &mut run => {
                panic!("node stopped before egress-only factory run was observed: {result:?}");
            }
        }

        run.await.expect("node should stop cleanly");
    })
    .await
    .expect("live node should run without external ingress before timeout");

    assert_eq!(handle.state(), NodeState::Stopped);
    msgbus::get_message_bus().borrow_mut().dispose();
    assert!(closed.load(Ordering::Relaxed));
}

#[rstest]
fn test_builder_with_external_msgbus_factory_rejects_injected_surfaces() {
    let (external_egress, _publications, _closed) = CapturingExternalEgress::new();

    let egress_factory = CapturingBackingFactory::new(
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(AtomicBool::new(false)),
        None,
    );

    let egress_error = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
        .unwrap()
        .with_external_msgbus_factory(Box::new(egress_factory))
        .with_external_msgbus_egress(Box::new(external_egress))
        .build()
        .expect_err("builder should reject factory plus injected egress");

    assert!(
        egress_error
            .to_string()
            .contains("cannot be combined with injected egress or ingress")
    );

    let (_tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);

    let ingress_factory = CapturingBackingFactory::new(
        Arc::new(Mutex::new(Vec::new())),
        Arc::new(AtomicBool::new(false)),
        None,
    );

    let ingress = CapturingExternalIngress::new(rx, Rc::new(Cell::new(false)));
    let ingress_error = LiveNode::builder(TraderId::from("TRADER-001"), Environment::Sandbox)
        .unwrap()
        .with_external_msgbus_factory(Box::new(ingress_factory))
        .with_external_ingress(Box::new(ingress))
        .build()
        .expect_err("builder should reject factory plus injected ingress");

    assert!(
        ingress_error
            .to_string()
            .contains("cannot be combined with injected egress or ingress")
    );
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_run_republishes_external_ingress_on_local_msgbus() {
    let quote = QuoteTick::default();
    let received = Rc::new(RefCell::new(Vec::<QuoteTick>::new()));
    let payload =
        Bytes::from(serde_json::to_vec(&quote).expect("QuoteTick should serialize as JSON"));
    let message = BusMessage::with_str_topic(
        "data.quotes.TEST",
        BusPayloadType::QuoteTick,
        payload,
        SerializationEncoding::Json,
    );
    let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
    let closed = Rc::new(Cell::new(false));
    let ingress = CapturingExternalIngress::new(rx, closed.clone());

    let config = LiveNodeConfig {
        environment: Environment::Sandbox,
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::from_millis(500),
        timeout_disconnection: Duration::from_millis(500),
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_external_ingress(Box::new(ingress))
        .build()
        .expect("node builds with external message bus ingress");
    let handle = node.handle();

    let handler = TypedHandler::from({
        let received = received.clone();
        move |quote: &QuoteTick| {
            received.borrow_mut().push(*quote);
        }
    });

    msgbus::subscribe_quotes("data.quotes.*".into(), handler, None);
    msgbus::get_message_bus()
        .borrow_mut()
        .add_streaming_type(BusPayloadType::QuoteTick);

    tokio::time::timeout(Duration::from_secs(30), async {
        let run = node.run();
        tokio::pin!(run);

        let drive = async {
            wait_until_async(|| async { handle.is_running() }, Duration::from_secs(10)).await;

            tx.send(message)
                .await
                .expect("external ingress receiver should be open");

            wait_until_async(
                || async { received.borrow().len() == 1 },
                Duration::from_secs(10),
            )
            .await;
            assert_eq!(*received.borrow(), vec![quote]);
            handle.stop();
        };

        tokio::select! {
            biased;

            () = drive => {}
            result = &mut run => {
                panic!("node stopped before external message was republished: {result:?}");
            }
        }

        run.await.expect("node should stop cleanly");
    })
    .await
    .expect("live node should republish ingress and stop before timeout");

    assert_eq!(handle.state(), NodeState::Stopped);
    assert!(closed.get());
    msgbus::get_message_bus().borrow_mut().dispose();
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_run_closes_external_ingress_when_receiver_closes() {
    let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
    let closed = Rc::new(Cell::new(false));
    let ingress = CapturingExternalIngress::new(rx, closed.clone());

    let config = LiveNodeConfig {
        environment: Environment::Sandbox,
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::from_millis(500),
        timeout_disconnection: Duration::from_millis(500),
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_external_ingress(Box::new(ingress))
        .build()
        .expect("node builds with external message bus ingress");
    let handle = node.handle();
    tokio::time::timeout(Duration::from_secs(30), async {
        let run = node.run();
        tokio::pin!(run);

        let drive = async {
            wait_until_async(|| async { handle.is_running() }, Duration::from_secs(10)).await;

            drop(tx);

            wait_until_async(|| async { closed.get() }, Duration::from_secs(10)).await;
            assert!(
                handle.is_running(),
                "node should keep running after ingress closes"
            );
            handle.stop();
        };

        tokio::select! {
            biased;

            () = drive => {}
            result = &mut run => {
                panic!("node stopped before ingress close was observed: {result:?}");
            }
        }

        run.await.expect("node should stop cleanly");
    })
    .await
    .expect("live node should close ingress and stop before timeout");

    assert_eq!(handle.state(), NodeState::Stopped);
}

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_run_aborts_startup_when_external_ingress_receiver_unavailable() {
    let closed = Rc::new(Cell::new(false));
    let ingress = FailingExternalIngress::new(closed.clone());

    let config = LiveNodeConfig {
        environment: Environment::Sandbox,
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        timeout_connection: Duration::from_millis(500),
        timeout_disconnection: Duration::from_millis(500),
        ..Default::default()
    };

    let mut node = LiveNodeBuilder::from_config(config)
        .unwrap()
        .with_external_ingress(Box::new(ingress))
        .build()
        .expect("node builds with external message bus ingress");
    let handle = node.handle();

    let err = node.run().await.expect_err("run should fail");

    assert!(
        err.to_string()
            .contains("external ingress receiver unavailable")
    );
    assert_eq!(handle.state(), NodeState::Stopped);
    assert!(closed.get());
}

#[derive(Debug)]
struct CapturingExternalIngress {
    rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
    closed: SharedClosed,
}

impl CapturingExternalIngress {
    fn new(rx: tokio::sync::mpsc::Receiver<BusMessage>, closed: SharedClosed) -> Self {
        Self {
            rx: Some(rx),
            closed,
        }
    }
}

impl MessageBusExternalIngress for CapturingExternalIngress {
    fn is_closed(&self) -> bool {
        self.closed.get()
    }

    fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        self.rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("external ingress receiver already taken"))
    }

    fn close(&mut self) {
        self.closed.set(true);
    }
}

#[derive(Debug)]
struct FailingExternalIngress {
    closed: SharedClosed,
}

impl FailingExternalIngress {
    fn new(closed: SharedClosed) -> Self {
        Self { closed }
    }
}

impl MessageBusExternalIngress for FailingExternalIngress {
    fn is_closed(&self) -> bool {
        self.closed.get()
    }

    fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        anyhow::bail!("external ingress receiver unavailable")
    }

    fn close(&mut self) {
        self.closed.set(true);
    }
}

struct CapturingBackingFactory {
    publications: Arc<Mutex<Vec<BusMessage>>>,
    closed: Arc<AtomicBool>,
    rx: Mutex<Option<tokio::sync::mpsc::Receiver<BusMessage>>>,
}

impl CapturingBackingFactory {
    fn new(
        publications: Arc<Mutex<Vec<BusMessage>>>,
        closed: Arc<AtomicBool>,
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
    ) -> Self {
        Self {
            publications,
            closed,
            rx: Mutex::new(rx),
        }
    }
}

impl Debug for CapturingBackingFactory {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(CapturingBackingFactory))
            .finish_non_exhaustive()
    }
}

impl MessageBusBackingFactory for CapturingBackingFactory {
    fn create(
        &self,
        _trader_id: TraderId,
        _instance_id: UUID4,
        _config: MessageBusConfig,
    ) -> anyhow::Result<Box<dyn MessageBusBacking>> {
        let rx = self.rx.lock().take();
        Ok(Box::new(CapturingBacking {
            publications: self.publications.clone(),
            closed: self.closed.clone(),
            rx,
        }))
    }
}

struct CapturingBacking {
    publications: Arc<Mutex<Vec<BusMessage>>>,
    closed: Arc<AtomicBool>,
    rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
}

impl MessageBusBacking for CapturingBacking {
    fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Relaxed)
    }

    fn publish(&self, message: BusMessage) {
        self.publications.lock().push(message);
    }

    fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
        self.rx
            .take()
            .ok_or_else(|| anyhow::anyhow!("external ingress receiver unavailable"))
    }

    fn close(&mut self) {
        self.closed.store(true, Ordering::Relaxed);
    }
}
