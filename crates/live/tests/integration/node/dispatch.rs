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

use std::{cell::RefCell, rc::Rc, time::Duration};

use nautilus_common::{
    live::runner::{get_data_event_sender, get_exec_event_sender},
    messages::system::{QueueCondition, QueueState, QueueStateChanged},
    msgbus::{self, MessagingSwitchboard, ShareableMessageHandler},
    runner::{SystemChannel, TimeEventMessage, get_data_cmd_sender, get_time_event_sender},
    testing::wait_until_async,
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::{
    config::{LiveExecutionEngineConfig, LiveNodeConfig, QueueMonitorConfig},
    node::LiveNode,
};
use nautilus_model::identifiers::TraderId;
use rstest::rstest;

use crate::dispatch_fixtures::{stub_data_command, stub_data_event, stub_exec_event};

#[rstest]
#[tokio::test(flavor = "current_thread")]
async fn test_run_publishes_queue_state_after_dispatch_sample() {
    let config = LiveNodeConfig {
        trader_id: TraderId::from("QUEUE-RUN-001"),
        queue_monitor: Some(QueueMonitorConfig {
            queue_depth_trigger: usize::MAX,
            queue_depth_clear: 0,
            mean_dispatch_ns_trigger: 1,
            mean_dispatch_ns_clear: 0,
        }),
        exec_engine: LiveExecutionEngineConfig {
            reconciliation: false,
            ..Default::default()
        },
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("QueueMonitorRunNode".to_string(), Some(config)).unwrap();
    let handle = node.handle();
    let received = Rc::new(RefCell::new(Vec::<QueueStateChanged>::new()));

    let handler = ShareableMessageHandler::from_typed({
        let received = received.clone();
        let stop_handle = handle.clone();

        move |event: &QueueStateChanged| {
            received.borrow_mut().push(event.clone());
            stop_handle.stop();
        }
    });

    msgbus::subscribe_any(
        MessagingSwitchboard::queue_state_changed_pattern(None),
        handler,
        None,
    );
    let drive_handle = handle.clone();

    let result = tokio::time::timeout(Duration::from_secs(5), async {
        let run = node.run();
        tokio::pin!(run);

        let drive = async move {
            wait_until_async(
                || async { drive_handle.is_running() },
                Duration::from_secs(2),
            )
            .await;
            get_data_event_sender().send(stub_data_event()).unwrap();
        };

        let (run_result, ()) = tokio::join!(run, drive);
        run_result
    })
    .await;

    assert!(
        result.is_ok(),
        "queue state event should arrive before timeout"
    );
    assert!(result.unwrap().is_ok(), "run() should succeed");
    let events = received.borrow();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].trader_id, TraderId::from("QUEUE-RUN-001"));
    assert_eq!(events[0].channel, SystemChannel::DataEvents);
    assert_eq!(events[0].condition, QueueCondition::Slow);
    assert_eq!(events[0].state, QueueState::Triggered);
    assert_eq!(events[0].queue_depth, 0);
    assert!(events[0].mean_dispatch_ns > 0);
    assert_ne!(events[0].event_id, UUID4::default());
    assert_ne!(events[0].ts_event, UnixNanos::default());
    assert_eq!(events[0].ts_init, events[0].ts_event);
    drop(events);
    msgbus::get_message_bus().borrow_mut().dispose();
}

#[rstest]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_run_dispatch_progress_under_load(
    #[values(false, true)] execution: bool,
    #[values(false, true)] slow: bool,
) {
    let config = LiveNodeConfig {
        delay_post_stop: Duration::ZERO,
        ..Default::default()
    };

    let mut node = LiveNode::build("FairDispatchNode".to_string(), Some(config)).unwrap();
    let handle = node.handle();
    let stop = handle.clone();

    let callback = TimeEventCallback::RustLocal(Rc::new(move |event| {
        if slow {
            // Make housekeeping ready after each timer callback
            std::thread::sleep(Duration::from_millis(110));
        }

        if event.ts_init == UnixNanos::from(8) {
            // Check progress before shutdown can drain the queues
            let snapshot = stop.metrics_snapshot();
            assert_eq!(snapshot.time_events.dispatched, 7);

            if execution {
                assert!((1..16).contains(&snapshot.exec_events.dispatched));
                assert_eq!(snapshot.data_events.dispatched, 0);
            } else {
                assert!(snapshot.data_events.dispatched > 0);
                assert_eq!(snapshot.data_commands.dispatched, 1);
            }

            stop.stop();
        }
    }));

    let drive = async {
        wait_until_async(|| async { handle.is_running() }, Duration::from_secs(2)).await;

        for ts in (1..=16_u64).map(UnixNanos::from) {
            get_time_event_sender().send(TimeEventMessage::new(
                TimeEvent::new("fairness".into(), UUID4::new(), ts, ts),
                callback.clone(),
            ));
            get_data_event_sender().send(stub_data_event()).unwrap();

            if execution {
                get_exec_event_sender().send(stub_exec_event()).unwrap();
            }
        }

        get_data_cmd_sender().execute(stub_data_command());
    };

    tokio::time::timeout(Duration::from_secs(5), async {
        let (result, ()) = tokio::join!(node.run(), drive);
        result.unwrap();
    })
    .await
    .unwrap();

    assert!(handle.should_stop());
}
