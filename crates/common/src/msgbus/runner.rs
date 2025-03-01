use core::ops::CoroutineState;
use std::{any::Any, fmt, fmt::Display, rc::Rc};

use crate::msgbus::*;

/// Publish message to given pattern.
///
/// This task will sequentially publish messages to all subscribers of the given
/// pattern. It tracks the index of the next subscriber to publish to. Note that
/// the subscribers dynamically change as new subscribers are registered and
/// unsubscribed from the pattern.
pub struct PublishTask {
    pattern: Ustr,
    msg: Rc<dyn Any>,
    idx: usize,
}

impl Debug for PublishTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "PublishTask(pattern: {}, idx: {})",
            self.pattern, self.idx
        )
    }
}

impl Display for PublishTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "pub::{}:{}", self.pattern, self.idx)
    }
}

impl PublishTask {
    pub fn new<T: AsRef<str>>(pattern: T, msg: Rc<dyn Any>) -> Self {
        let pattern = Ustr::from(pattern.as_ref());
        Self {
            pattern,
            msg,
            idx: 0,
        }
    }

    // Dummy implementation
    pub fn next_task(&mut self, msg_bus: &MessageBus) -> Option<SendTask> {
        // TODO: Fix this by getting matching subscriptions from the message bus
        let sub = msg_bus
            .subscriptions
            .iter()
            .filter(|(_sub, pattern)| pattern.contains(&self.pattern))
            .map(|(sub, _)| sub)
            .nth(self.idx);

        sub.map(|sub| {
            self.idx += 1;
            let handler_fn = (sub.handler_fn)();
            Some(SendTask::new(
                self.pattern.clone(),
                handler_fn,
                self.msg.clone(),
            ))
        })
        .flatten()
    }
}

/// Send message to a given endpoint with a single subscriber.
pub struct SendTask {
    endpoint: Ustr,
    coro: HandlerCoroutine,
    msg: Rc<dyn Any>,
}

impl Debug for SendTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SendTask {{endpoint:{}}}", self.endpoint)
    }
}

impl Display for SendTask {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "send::{}", self.endpoint)
    }
}

impl SendTask {
    pub fn new<T: AsRef<str>>(endpoint: T, coro: HandlerCoroutine, msg: Rc<dyn Any>) -> Self {
        let endpoint = Ustr::from(endpoint.as_ref());
        Self {
            endpoint,
            coro,
            msg,
        }
    }

    pub fn resume(&mut self) -> CoroutineState<Command, ()> {
        let msg = self.msg.clone();
        self.coro.as_mut().resume(msg)
    }
}

pub enum Task {
    Send(SendTask),
    Publish(PublishTask),
}

impl Display for Task {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Task::Send(send) => writeln!(f, "{}", send),
            Task::Publish(publish) => writeln!(f, "{}", publish),
        }
    }
}

#[derive(Default)]
pub struct TaskRunner {
    pub tasks: Vec<Task>,
    pub msg_bus: MessageBus,
}

impl Display for TaskRunner {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "TaskRunner")?;
        writeln!(f, "tasks:")?;
        for task in &self.tasks {
            writeln!(f, "{}", task)?;
        }
        Ok(())
    }
}

impl From<MessageBus> for TaskRunner {
    fn from(msg_bus: MessageBus) -> Self {
        Self {
            tasks: Vec::new(),
            msg_bus,
        }
    }
}

impl TaskRunner {
    pub fn push(&mut self, task: Task) {
        self.tasks.push(task);
    }

    pub fn pop(&mut self) -> Option<Task> {
        self.tasks.pop()
    }

    pub fn step(&mut self) {
        match self.tasks.last_mut() {
            Some(Task::Send(send)) => {
                match send.resume() {
                    CoroutineState::Yielded(cmd) => {
                        // Process the yielded command.
                        match cmd {
                            Command::Send { topic, msg } => {
                                if let Some(sub) = self.msg_bus.endpoints.get(&topic) {
                                    let coro = (sub.handler_fn)();
                                    self.push(Task::Send(SendTask::new(topic, coro, msg)));
                                }
                            }
                            Command::Register(subscription) => {
                                self.msg_bus.register(subscription);
                            }
                            Command::Deregister(topic) => {
                                self.msg_bus.deregister(&topic);
                            }
                            Command::Subscribe(subscription) => {
                                self.msg_bus.subscribe(subscription);
                            }
                            Command::Unsubscribe((topic, handler_id)) => {
                                self.msg_bus.unsubscribe(&topic, &handler_id);
                            }
                            Command::Publish { pattern, msg } => {
                                self.push(Task::Publish(PublishTask::new(pattern, msg)));
                            }
                        }
                    }
                    CoroutineState::Complete(_) => {
                        self.tasks.pop();
                    }
                }
            }
            Some(Task::Publish(publish)) => match publish.next_task(&self.msg_bus) {
                Some(send) => self.push(Task::Send(send)),
                None => {
                    self.tasks.pop();
                }
            },
            None => {}
        }
    }

    pub fn run(&mut self) {
        while !self.tasks.is_empty() {
            self.step();
        }
    }
}
