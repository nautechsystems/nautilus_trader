// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(dead_code)]
#![allow(unused_variables)]
#![allow(unused_imports)]

use std::{cell::RefCell, net::SocketAddr, pin::Pin, rc::Rc};

use data_client::MockDataClient;
use futures::{Stream, stream::SelectAll};
use nautilus_common::{
    cache::Cache,
    clock::{Clock, LiveClock},
    messages::data::DataResponse,
    msgbus::{
        handler::{MessageHandler, ShareableMessageHandler},
        register,
    },
};
use nautilus_data::{
    client::{DataClient, DataClientAdapter},
    engine::{DataEngine, SubscriptionCommandHandler},
};
use nautilus_model::identifiers::Venue;
use tokio_stream::StreamExt;
use ustr::Ustr;

pub mod big_brain_actor;
pub mod data_client;
pub mod http_server;
pub mod websocket_server;

pub async fn init_data_engine(
    http_address: SocketAddr,
    websocket_address: SocketAddr,
) -> (
    Pin<Box<dyn Stream<Item = DataResponse>>>,
    Pin<Box<dyn Stream<Item = i32>>>,
) {
    let (client, http_stream, websocket_stream) =
        MockDataClient::start(http_address, websocket_address).await;
    let client: Box<dyn DataClient> = Box::new(client);
    let clock: Rc<RefCell<dyn Clock>> = Rc::new(RefCell::new(LiveClock::new()));

    let adapter = DataClientAdapter::new(
        client.client_id(),
        Venue::from_str_unchecked("yooohooo"),
        false,
        false,
        client,
        clock.clone(),
    );
    let cache = Rc::new(RefCell::new(Cache::new(None, None)));

    let mut engine = DataEngine::new(clock, cache, None);
    engine.register_client(adapter, None);

    let engine = Rc::new(RefCell::new(engine));
    let handler = SubscriptionCommandHandler {
        id: Ustr::from("data_engine_handler"),
        engine_ref: engine,
    };

    let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
    register("data_engine", handler);

    (http_stream, websocket_stream)
}

#[derive(Default)]
pub struct LiveRunner {
    data_response_stream: SelectAll<Pin<Box<dyn Stream<Item = DataResponse>>>>,
    message_stream: SelectAll<Pin<Box<dyn Stream<Item = i32>>>>,
}

impl LiveRunner {
    pub fn new_add_data_response_stream(
        &mut self,
        stream: Pin<Box<dyn Stream<Item = DataResponse>>>,
    ) {
        self.data_response_stream.push(stream);
    }

    pub fn new_message_stream(&mut self, stream: Pin<Box<dyn Stream<Item = i32>>>) {
        self.message_stream.push(stream);
    }

    pub async fn run(&mut self) {
        loop {
            // TODO: push decoding logic into data client
            tokio::select! {
                data_response = self.data_response_stream.next() => {
                    if let Some(DataResponse::Data(custom_data_response)) = data_response {
                            println!("Received custom data response: {custom_data_response:?}");
                            let value = custom_data_response.data.downcast_ref::<i32>().copied().unwrap();
                            nautilus_common::msgbus::response(&custom_data_response.correlation_id, &value);
                    }
                }
                message = self.message_stream.next() => {
                    if let Some(message) = message {
                        nautilus_common::msgbus::send(&Ustr::from("negative_stream"), &message);
                    }
                }
            }
        }
    }
}
