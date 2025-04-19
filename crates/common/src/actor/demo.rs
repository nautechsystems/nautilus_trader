use futures::Stream;
use futures::StreamExt;
use futures::stream::SelectAll;
use std::collections::HashMap;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, UnboundedReceiver, UnboundedSender};
use tokio::task;

use std::any::Any;

use super::registry::get_actor_unchecked;
use crate::actor::Actor;
use ustr::Ustr;

/// Control messages that can be sent to the data streams
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Stop the stream
    Stop,
    /// Skip the next n values
    Skip(i32),
}

#[allow(missing_debug_implementations)]
pub struct DataClient {
    /// The unique identifier for the stream
    id: usize,
    /// The current value
    value: i32,
    /// Whether the stream has been stopped
    stopped: bool,
    /// Receiver for control messages
    control_rx: UnboundedReceiver<ControlMessage>,
    /// Sleep duration between emitting values
    sleep_duration: Duration,
    /// Increasing or decreasing stream
    increasing: bool,
}

impl DataClient {
    /// Create a new controlled stream
    pub fn new(
        id: usize,
        initial_value: i32,
        sleep_duration: Duration,
        increasing: bool,
    ) -> (Self, UnboundedSender<ControlMessage>) {
        let (tx, rx) = mpsc::unbounded_channel();

        (
            Self {
                id,
                value: initial_value,
                stopped: false,
                control_rx: rx,
                sleep_duration,
                increasing,
            },
            tx,
        )
    }
}

impl Stream for DataClient {
    type Item = (usize, i32);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Check for control messages - use poll_recv instead of try_recv
        // try_recv returns immediately with an error if no message is available
        // poll_recv will properly register with the channel to be notified when messages arrive
        while let Poll::Ready(Some(msg)) = Pin::new(&mut self.control_rx).poll_recv(cx) {
            match msg {
                ControlMessage::Stop => {
                    println!("Stopping stream {}", self.id);
                    self.stopped = true;
                }
                ControlMessage::Skip(n) => {
                    println!("Skipping {} values for stream {}", n, self.id);
                    if self.increasing {
                        self.value += n;
                    } else {
                        self.value -= n;
                    }
                }
            }
        }

        // If stopped, return None to end the stream
        if self.stopped {
            return Poll::Ready(None);
        }

        // Get the current value
        let current_value = self.value;

        // Compute the next value
        if self.increasing {
            self.value += 1;
        } else {
            self.value -= 1;
        }

        // Sleep before next value
        let sleep_duration = self.sleep_duration;
        let waker = cx.waker().clone();

        // Spawn a task to sleep and then wake the stream
        task::spawn(async move {
            tokio::time::sleep(sleep_duration).await;
            waker.wake()
        });

        Poll::Ready(Some((self.id, current_value)))
    }
}

#[allow(missing_debug_implementations)]
/// A runner that multiplexes values from multiple streams
pub struct StreamRunner {
    index: usize,
    streams: SelectAll<Pin<Box<dyn Stream<Item = (usize, i32)>>>>,
    runtime: Runtime,
}

impl StreamRunner {
    /// Create a new stream runner
    pub fn new(runtime: Runtime) -> Self {
        Self {
            index: 0,
            streams: SelectAll::new(),
            runtime,
        }
    }

    /// Add a stream to the runner
    pub fn add_stream<S>(&mut self, stream: S)
    where
        S: Stream<Item = (usize, i32)> + Send + 'static,
    {
        self.streams.push(Box::pin(stream));
    }
}

impl Iterator for StreamRunner {
    type Item = (usize, i32);

    // TODO: Eagerly process the stream instead of waiting for the next() call
    fn next(&mut self) -> Option<Self::Item> {
        self.runtime.block_on(self.streams.next())
    }
}

#[derive(Debug)]
/// A data engine that manages control channels for streams
struct DataEngine {
    channel_map: HashMap<usize, UnboundedSender<ControlMessage>>,
}

impl DataEngine {
    pub fn new() -> Self {
        Self {
            channel_map: HashMap::new(),
        }
    }

    pub fn add_stream(&mut self, id: usize, tx: UnboundedSender<ControlMessage>) {
        self.channel_map.insert(id, tx);
    }

    pub fn send_control_message(&self, id: usize, message: ControlMessage) {
        if let Some(tx) = self.channel_map.get(&id) {
            println!(
                "Sending control message for stream {} to data engine: {:?}",
                id, message
            );
            if let Err(e) = tx.send(message) {
                println!("Error sending control message: {:?}", e);
            }
        }
    }
}

impl Actor for DataEngine {
    fn id(&self) -> Ustr {
        "data_engine".into()
    }

    fn handle(&mut self, msg: &dyn Any) {}

    fn as_any(&self) -> &dyn Any {
        self
    }
}

fn data_engine_handler(msg: &(usize, ControlMessage)) {
    let (id, msg) = msg;
    let actor_id = Ustr::from("data_engine");
    let data_engine = get_actor_unchecked::<DataEngine>(&actor_id);
    println!(
        "Sending control message for stream {} to data engine: {:?}",
        id, msg
    );
    data_engine.send_control_message(*id, msg.clone());
}

fn demo_handler(msg: &(usize, i32)) {
    let (idx, value) = msg;
    println!("Stream {}: {}", idx, value);

    if *idx == 0 && *value > 5 {
        println!("Positive stream crossed 5, skipping 5 values on positive stream");
        crate::msgbus::send(
            &"data_engine_control_message".into(),
            &(*idx, ControlMessage::Skip(5)),
        );
    }

    if *idx == 1 && *value < -5 {
        println!("Negative stream crossed -5, skipping 5 values on negative stream");
        crate::msgbus::send(
            &"data_engine_control_message".into(),
            &(*idx, ControlMessage::Skip(5)),
        );
    }

    // Stop either stream if it crosses 12 (absolute value)
    if value.abs() > 10 {
        println!("Stream {} crossed absolute value 10, stopping", idx);
        crate::msgbus::send(
            &"data_engine_control_message".into(),
            &(*idx, ControlMessage::Stop),
        );
    }
}

mod tests {
    use super::*;
    use crate::actor::registry::register_actor;
    use crate::msgbus::{
        self, MessageBus,
        handler::{MessageHandler, ShareableMessageHandler, TypedMessageHandler},
        set_message_bus,
    };
    use std::cell::{RefCell, UnsafeCell};
    use std::rc::Rc;

    /// Run the demo synchronously
    #[test]
    pub fn run_demo_sync() {
        // Create a runtime for initialization
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let data_engine = Rc::new(UnsafeCell::new(DataEngine::new()));
        register_actor(data_engine.clone());

        let msgbus = Rc::new(RefCell::new(MessageBus::default()));
        set_message_bus(msgbus.clone());

        // Register data engine control message handler
        let handler = TypedMessageHandler::from(data_engine_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        msgbus::register("data_engine_control_message", handler);

        // Register demo actor core logic handler
        let handler = TypedMessageHandler::from(demo_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        msgbus::register("demo_handler", handler);

        // Initialize the streams inside the runtime
        let (increasing_stream, increasing_tx) =
            runtime.block_on(async { DataClient::new(0, 0, Duration::from_millis(500), true) });

        let (decreasing_stream, decreasing_tx) =
            runtime.block_on(async { DataClient::new(1, 0, Duration::from_millis(800), false) });

        let actor_id = Ustr::from("data_engine");
        let data_engine = get_actor_unchecked::<DataEngine>(&actor_id);
        data_engine.add_stream(0, increasing_tx);
        data_engine.add_stream(1, decreasing_tx);

        assert!(data_engine.channel_map.len() == 2);
        assert!(data_engine.channel_map.get(&0).is_some());
        assert!(data_engine.channel_map.get(&1).is_some());

        // Create the runner
        let mut runner = StreamRunner::new(runtime);
        runner.add_stream(increasing_stream);
        runner.add_stream(decreasing_stream);

        println!("Running demo synchronously");

        // Process the values synchronously
        for (idx, value) in runner {
            msgbus::send(&"demo_handler".into(), &(idx, value));
        }

        println!("All streams have ended");
    }
}
