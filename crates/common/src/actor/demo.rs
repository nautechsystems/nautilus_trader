use futures::Stream;
use futures::StreamExt;
use futures::executor::block_on_stream;
use futures::stream::SelectAll;
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};
use std::thread;
use std::time::Duration;
use tokio::runtime::Runtime;
use tokio::sync::mpsc::{self, Receiver, Sender, UnboundedReceiver, UnboundedSender};
use tokio::task;

/// Control messages that can be sent to the data streams
#[derive(Debug, Clone)]
pub enum ControlMessage {
    /// Stop the stream
    Stop,
    /// Skip the next n values
    Skip(i32),
}

/// A data stream that can be controlled via messages
pub struct ControlledStream {
    /// The unique identifier for the stream
    id: usize,
    /// The current value
    value: i32,
    /// Whether the stream has been stopped
    stopped: bool,
    /// Receiver for control messages
    control_rx: UnboundedReceiver<ControlMessage>,
    /// Waker to wake the stream when a control message is received
    waker: Option<Waker>,
    /// Sleep duration between emitting values
    sleep_duration: Duration,
    /// Increasing or decreasing stream
    increasing: bool,
}

impl ControlledStream {
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
                waker: None,
                sleep_duration,
                increasing,
            },
            tx,
        )
    }
}

impl Stream for ControlledStream {
    type Item = (usize, i32);

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        // Store the waker for later use
        self.waker = Some(cx.waker().clone());

        // Check for control messages
        match self.control_rx.try_recv() {
            Ok(ControlMessage::Stop) => {
                self.stopped = true;
            }
            Ok(ControlMessage::Skip(n)) => {
                if self.increasing {
                    self.value += n;
                } else {
                    self.value -= n;
                }
            }
            Err(_) => {}
        }

        // If stopped, return None to end the stream
        if self.stopped {
            return Poll::Ready(None);
        }

        // Get the current value
        let current_value = self.value.clone();

        // Compute the next value
        if self.increasing {
            self.value += 1;
        } else {
            self.value -= 1;
        }

        // Sleep before next value
        let sleep_duration = self.sleep_duration;
        let waker = self.waker.clone();

        // Spawn a task to sleep and then wake the stream
        task::spawn(async move {
            tokio::time::sleep(sleep_duration).await;
            if let Some(waker) = waker {
                waker.wake();
            }
        });

        Poll::Ready(Some((self.id, current_value)))
    }
}

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
            let _ = tx.send(message);
        }
    }
}

mod tests {
    use super::*;

    /// Run the demo synchronously
    #[test]
    pub fn run_demo_sync() {
        // Create a runtime for initialization
        let runtime = tokio::runtime::Runtime::new().unwrap();

        // Initialize the streams inside the runtime
        let (increasing_stream, increasing_tx) = runtime
            .block_on(async { ControlledStream::new(0, 0, Duration::from_millis(500), true) });

        let (decreasing_stream, decreasing_tx) = runtime
            .block_on(async { ControlledStream::new(1, 0, Duration::from_millis(800), false) });

        let mut data_engine = DataEngine::new();
        data_engine.add_stream(0, increasing_tx);
        data_engine.add_stream(1, decreasing_tx);

        // Create the runner
        let mut runner = StreamRunner::new(runtime);
        runner.add_stream(increasing_stream);
        runner.add_stream(decreasing_stream);

        println!("Running demo synchronously");

        // Process the values synchronously
        for (idx, value) in runner {
            println!("Stream {}: {}", idx, value);

            if idx == 0 && value > 5 {
                println!("Positive stream crossed 5, skipping 5 values on positive stream");
                data_engine.send_control_message(0, ControlMessage::Skip(5));
            }

            if idx == 1 && value < -5 {
                println!("Negative stream crossed -5, skipping 5 values on negative stream");
                data_engine.send_control_message(1, ControlMessage::Skip(5));
            }

            // Stop either stream if it crosses 12 (absolute value)
            if value.abs() > 10 {
                println!("Stream {} crossed absolute value 10, stopping", idx);
                data_engine.send_control_message(idx, ControlMessage::Stop);
            }
        }

        println!("All streams have ended");
    }
}
