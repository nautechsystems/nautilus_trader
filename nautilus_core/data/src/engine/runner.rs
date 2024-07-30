use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use nautilus_common::{
    component::Running,
    messages::data::{DataClientResponse, DataResponse},
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::DataEngine;

pub trait Runner {
    type Sender;

    fn new() -> Self;
    fn run(&mut self, engine: &DataEngine<Running>);
    fn get_sender(&self) -> Self::Sender;
}

pub trait SendResponse {
    fn send(&self, resp: DataResponse);
}

pub struct LiveRunner {
    resp_tx: UnboundedSender<DataClientResponse>,
    resp_rx: UnboundedReceiver<DataClientResponse>,
}

impl Runner for LiveRunner {
    type Sender = UnboundedSender<DataClientResponse>;

    fn new() -> Self {
        let (resp_tx, resp_rx) = tokio::sync::mpsc::unbounded_channel::<DataClientResponse>();
        Self { resp_tx, resp_rx }
    }

    fn run(&mut self, engine: &DataEngine<Running>) {
        while let Some(resp) = self.resp_rx.blocking_recv() {
            match resp {
                DataClientResponse::DataResponse(data_resp) => engine.response(data_resp),
                DataClientResponse::Data(data) => engine.process(data),
            }
        }
    }

    fn get_sender(&self) -> Self::Sender {
        self.resp_tx.clone()
    }
}

pub type DataResponseQueue = Rc<RefCell<VecDeque<DataClientResponse>>>;

pub struct BacktestRunner {
    queue: DataResponseQueue,
}

impl Runner for BacktestRunner {
    type Sender = DataResponseQueue;

    fn new() -> Self {
        Self {
            queue: Rc::new(RefCell::new(VecDeque::new())),
        }
    }

    fn run(&mut self, engine: &DataEngine<Running>) {
        while let Some(resp) = self.queue.as_ref().borrow_mut().pop_front() {
            match resp {
                DataClientResponse::DataResponse(data_resp) => engine.response(data_resp),
                DataClientResponse::Data(data) => engine.process(data),
            }
        }
    }

    fn get_sender(&self) -> Self::Sender {
        self.queue.clone()
    }
}
