use std::cell::UnsafeCell;
use std::rc::Rc;

use nautilus_common::actor::registry::register_actor;
use nautilus_common::msgbus::handler::MessageHandler;
use nautilus_common::msgbus::handler::ShareableMessageHandler;
use nautilus_common::msgbus::handler::TypedMessageHandler;
use nautilus_common::msgbus::register;
use nautilus_demo::big_brain_actor::BigBrainActor;
use nautilus_demo::big_brain_actor::negative_handler;
use nautilus_demo::data_client::MockNetworkDataClient;
use nautilus_demo::http_server::start_positive_stream_http_server;
use nautilus_demo::websocket_server::NegativeStreamServer;
use tokio_tungstenite::tungstenite::Message;

use futures::StreamExt;
use futures::stream::SelectAll;
use nautilus_common::messages::data::DataResponse;
use tokio_stream::wrappers::ReceiverStream;
use tokio_stream::wrappers::UnboundedReceiverStream;
use ustr::Ustr;

async fn main_logic() {
    let http_address = start_positive_stream_http_server().await.unwrap();
    let websocket_server = NegativeStreamServer::setup().await;

    // Initialize data client with http and websocket streams
    let (data_client, http_stream, websocket_stream) =
        MockNetworkDataClient::start(http_address, websocket_server.address).await;

    // Register data client as actor
    let actor_data_client = Rc::new(UnsafeCell::new(data_client));
    register_actor(actor_data_client);
    MockNetworkDataClient::register_message_handlers();

    // Initialize big brain actor
    let big_brain_actor = BigBrainActor::new();
    let big_brain_actor = Rc::new(UnsafeCell::new(big_brain_actor));
    register_actor(big_brain_actor);
    BigBrainActor::register_message_handlers();

    // initialize data streams
    let mut data_response_select_all: SelectAll<UnboundedReceiverStream<DataResponse>> =
        SelectAll::new();
    data_response_select_all.push(http_stream);

    let mut websocket_message_stream_select_all: SelectAll<ReceiverStream<Message>> =
        SelectAll::new();
    websocket_message_stream_select_all.push(websocket_stream);

    tokio::select! {
        data_response = data_response_select_all.next() => {
            if let Some(data_response) = data_response {
                println!("Received data response: {:?}", data_response);
                nautilus_common::msgbus::response(&data_response.correlation_id, &data_response.data);
            }
        }
        message = websocket_message_stream_select_all.next() => {
            if let Some(message) = message {
                match message {
                    Message::Text(text) => {
                        println!("Received text message: {}", text);
                        let data = text.parse::<i32>().unwrap();
                        nautilus_common::msgbus::send(&Ustr::from("negative_stream"), &data);
                    }
                    _ => {}
                }
            }
        }
    }
}

pub fn main() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(main_logic());
}
