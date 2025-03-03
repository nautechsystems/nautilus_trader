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

pub trait Actor {
    fn id(&self) -> String;
}

#[derive(Default)]
pub struct TaskRunner {
    pub actors: HashMap<String, Rc<dyn Actor>>,
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
            actors: HashMap::new(),
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

    pub fn store_actor(&mut self, actor: Rc<dyn Actor>) {
        self.actors.insert(actor.id(), actor);
    }
}

#[cfg(test)]
mod property_tests {
    use std::{cell::RefCell, rc::Rc};

    use super::*;
    use crate::msgbus::tests::stub_msgbus;

    // Simplified trace events
    #[derive(Debug, Clone, PartialEq)]
    enum TraceEvent {
        Enter(String), // Enter handler with ID
        Exit(String),  // Exit handler with ID
    }

    // Simple actions an actor can perform
    #[derive(Debug, Clone, PartialEq)]
    enum ActorAction {
        Send(String),    // Send to topic
        Publish(String), // Publish to pattern
    }

    // Function to verify a trace is well-formed (proper nesting)
    fn is_well_formed(trace: &[TraceEvent]) -> bool {
        let mut stack = Vec::new();

        for event in trace {
            match event {
                TraceEvent::Enter(id) => {
                    stack.push(id.clone());
                }
                TraceEvent::Exit(id) => {
                    if stack.pop() != Some(id.clone()) {
                        return false; // Mismatched exit
                    }
                }
            }
        }

        stack.is_empty() // Stack should be empty at the end
    }

    // Create a handler that executes a sequence of actions
    fn create_actor_handler(
        id: String,
        topic: String,
        actions: Vec<ActorAction>,
        trace: Rc<RefCell<Vec<TraceEvent>>>,
    ) -> Subscription {
        let id_clone = id.clone();
        let handler_fn = Rc::new(move || {
            let id = id.clone();
            let trace = trace.clone();
            let actions = actions.clone();

            Box::pin(
                #[coroutine]
                static move |_msg: Rc<dyn Any>| {
                    // Record entry
                    trace.borrow_mut().push(TraceEvent::Enter(id.clone()));

                    // Execute each action in sequence
                    for action in &actions {
                        match action {
                            ActorAction::Send(to_topic) => {
                                yield Command::Send {
                                    topic: to_topic.into(),
                                    msg: Rc::new(()),
                                };
                            }
                            ActorAction::Publish(pattern) => {
                                yield Command::Publish {
                                    pattern: pattern.clone(),
                                    msg: Rc::new(()),
                                };
                            }
                        }
                    }

                    // Record exit
                    trace.borrow_mut().push(TraceEvent::Exit(id.clone()));
                },
            ) as HandlerCoroutine
        });

        Subscription {
            topic: topic.into(),
            handler_fn,
            handler_id: id_clone.into(),
            priority: 0,
        }
    }

    // Test for static chain: A -> B -> C
    #[test]
    fn test_static_chain() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let msgbus = stub_msgbus();
        let mut runner: TaskRunner = msgbus.into();

        // Define actions for each actor
        let c_actions: Vec<ActorAction> = vec![];
        let b_actions = vec![ActorAction::Send("topic_c".to_string())];
        let a_actions = vec![ActorAction::Send("topic_b".to_string())];

        // Register all handlers
        runner.msg_bus.register(create_actor_handler(
            "C".to_string(),
            "topic_c".to_string(),
            c_actions,
            trace.clone(),
        ));

        runner.msg_bus.register(create_actor_handler(
            "B".to_string(),
            "topic_b".to_string(),
            b_actions,
            trace.clone(),
        ));

        let sub_a = create_actor_handler(
            "A".to_string(),
            "topic_a".to_string(),
            a_actions,
            trace.clone(),
        );
        runner.msg_bus.register(sub_a.clone());

        // Start with A
        runner.push(Task::Send(SendTask::new(
            "topic_a".to_string(),
            (sub_a.handler_fn)(),
            Rc::new(()),
        )));

        // Run and verify
        runner.run();

        // Expected trace: A enters, B enters, C enters, C exits, B exits, A exits
        let expected_trace = vec![
            TraceEvent::Enter("A".to_string()),
            TraceEvent::Enter("B".to_string()),
            TraceEvent::Enter("C".to_string()),
            TraceEvent::Exit("C".to_string()),
            TraceEvent::Exit("B".to_string()),
            TraceEvent::Exit("A".to_string()),
        ];

        assert!(
            is_well_formed(&trace.borrow()),
            "Trace is not well-formed: {:?}",
            *trace.borrow()
        );

        assert_eq!(
            *trace.borrow(),
            expected_trace,
            "Trace mismatch: {:?}",
            *trace.borrow()
        );
    }

    // Test for tree structure: A -> (B, C), B -> (D, E)
    #[test]
    fn test_tree_structure() {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let mut runner: TaskRunner = stub_msgbus().into();

        // Define actions for each actor
        let d_actions: Vec<ActorAction> = vec![];
        let e_actions: Vec<ActorAction> = vec![];
        let c_actions: Vec<ActorAction> = vec![];
        let b_actions = vec![
            ActorAction::Send("topic_d".to_string()),
            ActorAction::Send("topic_e".to_string()),
        ];
        let a_actions = vec![
            ActorAction::Send("topic_b".to_string()),
            ActorAction::Send("topic_c".to_string()),
        ];

        // Register all handlers
        runner.msg_bus.register(create_actor_handler(
            "D".to_string(),
            "topic_d".to_string(),
            d_actions,
            trace.clone(),
        ));

        runner.msg_bus.register(create_actor_handler(
            "E".to_string(),
            "topic_e".to_string(),
            e_actions,
            trace.clone(),
        ));

        runner.msg_bus.register(create_actor_handler(
            "C".to_string(),
            "topic_c".to_string(),
            c_actions,
            trace.clone(),
        ));

        runner.msg_bus.register(create_actor_handler(
            "B".to_string(),
            "topic_b".to_string(),
            b_actions,
            trace.clone(),
        ));

        let sub_a = create_actor_handler(
            "A".to_string(),
            "topic_a".to_string(),
            a_actions,
            trace.clone(),
        );
        runner.msg_bus.register(sub_a.clone());

        // Start with A
        runner.push(Task::Send(SendTask::new(
            "topic_a".to_string(),
            (sub_a.handler_fn)(),
            Rc::new(()),
        )));

        // Run and verify
        runner.run();

        // Expected trace: A enters, B enters, D enters, D exits, E enters, E exits, B exits, C enters, C exits, A exits
        let expected_trace = vec![
            TraceEvent::Enter("A".to_string()),
            TraceEvent::Enter("B".to_string()),
            TraceEvent::Enter("D".to_string()),
            TraceEvent::Exit("D".to_string()),
            TraceEvent::Enter("E".to_string()),
            TraceEvent::Exit("E".to_string()),
            TraceEvent::Exit("B".to_string()),
            TraceEvent::Enter("C".to_string()),
            TraceEvent::Exit("C".to_string()),
            TraceEvent::Exit("A".to_string()),
        ];

        assert!(
            is_well_formed(&trace.borrow()),
            "Trace is not well-formed: {:?}",
            *trace.borrow()
        );

        assert_eq!(
            *trace.borrow(),
            expected_trace,
            "Trace mismatch: {:?}",
            *trace.borrow()
        );
    }
}
