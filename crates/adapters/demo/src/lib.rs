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

use std::{cell::RefCell, net::SocketAddr, pin::Pin, rc::Rc};

use data_client::MockDataClient;
use futures::{Stream, stream::SelectAll};
use nautilus_common::{
    cache::Cache,
    clock::{Clock, LiveClock},
    messages::data::{DataCommand, DataResponse},
    msgbus::{
        self,
        handler::{ShareableMessageHandler, TypedMessageHandler},
    },
};
use nautilus_data::{
    client::{DataClient, DataClientAdapter},
    engine::DataEngine,
};
use nautilus_model::identifiers::Venue;
use tokio_stream::StreamExt;

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
        Some(Venue::from_str_unchecked("DEMO")),
        false,
        false,
        client,
    );
    let cache = Rc::new(RefCell::new(Cache::new(None, None)));

    let mut data_engine = DataEngine::new(clock, cache, None);
    data_engine.register_client(adapter, None);
    let data_engine = Rc::new(RefCell::new(data_engine));

    let data_engine_clone = data_engine;
    let _handler = ShareableMessageHandler(Rc::new(TypedMessageHandler::from(
        move |cmd: &DataCommand| data_engine_clone.borrow_mut().execute(cmd),
    )));

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
        let endpoint = "negative_stream".into();

        loop {
            // TODO: push decoding logic into data client
            tokio::select! {
                data_response = self.data_response_stream.next() => {
                    if let Some(DataResponse::Data(custom_data_response)) = data_response {
                            println!("Received custom data response: {custom_data_response:?}");
                            let value = custom_data_response.data.downcast_ref::<i32>().copied().unwrap();
                            msgbus::response(&custom_data_response.correlation_id, &value);
                    }
                }
                message = self.message_stream.next() => {
                    if let Some(message) = message {
                        msgbus::send(endpoint, &message);
                    }
                }
            }
        }
    }
}
