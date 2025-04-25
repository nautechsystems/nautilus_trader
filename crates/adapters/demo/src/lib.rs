use std::{cell::RefCell, net::SocketAddr, rc::Rc};
use tokio_stream::StreamExt;

use data_client::MockDataClient;
use futures::stream::SelectAll;
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
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

pub mod big_brain_actor;
pub mod data_client;
pub mod http_server;
pub mod websocket_server;

pub async fn init_data_engine(
    http_address: SocketAddr,
    websocket_address: SocketAddr,
) -> (
    tokio_stream::wrappers::UnboundedReceiverStream<DataResponse>,
    tokio_stream::wrappers::ReceiverStream<tokio_tungstenite::tungstenite::Message>,
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
        engine_ref: engine.clone(),
    };

    let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
    register("data_engine", handler);

    (http_stream, websocket_stream)
}

#[derive(Default)]
pub struct LiveRunner {
    data_response_stream: SelectAll<UnboundedReceiverStream<DataResponse>>,
    message_stream: SelectAll<ReceiverStream<Message>>,
}

impl LiveRunner {
    pub fn new_add_data_response_stream(&mut self, stream: UnboundedReceiverStream<DataResponse>) {
        self.data_response_stream.push(stream);
    }

    pub fn new_message_stream(&mut self, stream: ReceiverStream<Message>) {
        self.message_stream.push(stream);
    }

    pub async fn run(&mut self) {
        loop {
            // TODO: push decoding logic into data client
            tokio::select! {
                data_response = self.data_response_stream.next() => {
                    if let Some(data_response) = data_response {
                        println!("Received data response: {:?}", data_response);
                        let value = data_response.data.downcast_ref::<i32>().copied().unwrap();
                        nautilus_common::msgbus::response(&data_response.correlation_id, &value);
                    }
                }
                message = self.message_stream.next() => {
                    if let Some(message) = message {
                        if let Message::Text(text) = message {
                            println!("Received text message: {}", text);
                            let data = text.parse::<i32>().unwrap();
                            nautilus_common::msgbus::send(&Ustr::from("negative_stream"), &data);
                        }
                    }
                }
            }
        }
    }
}
