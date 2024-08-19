// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use std::{cell::RefCell, collections::VecDeque, rc::Rc};

use nautilus_common::messages::data::{DataClientResponse, DataResponse};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};

use super::DataEngine;

pub trait Runner {
    type Sender;

    fn new() -> Self;
    fn run(&mut self, engine: &mut DataEngine);
    fn get_sender(&self) -> Self::Sender;
}

pub trait SendResponse {
    fn send(&self, resp: DataResponse);
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

    fn run(&mut self, engine: &mut DataEngine) {
        while let Some(resp) = self.queue.as_ref().borrow_mut().pop_front() {
            match resp {
                DataClientResponse::Response(resp) => engine.response(resp),
                DataClientResponse::Data(data) => engine.process_data(data),
            }
        }
    }

    fn get_sender(&self) -> Self::Sender {
        self.queue.clone()
    }
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

    fn run(&mut self, engine: &mut DataEngine) {
        while let Some(resp) = self.resp_rx.blocking_recv() {
            match resp {
                DataClientResponse::Response(resp) => engine.response(resp),
                DataClientResponse::Data(data) => engine.process_data(data),
            }
        }
    }

    fn get_sender(&self) -> Self::Sender {
        self.resp_tx.clone()
    }
}
