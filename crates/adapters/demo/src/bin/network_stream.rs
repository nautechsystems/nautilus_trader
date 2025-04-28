use std::cell::UnsafeCell;
use std::rc::Rc;

use nautilus_common::actor::registry::register_actor;
use nautilus_common::testing::init_logger_for_testing;
use nautilus_demo::LiveRunner;
use nautilus_demo::big_brain_actor::BigBrainActor;
use nautilus_demo::http_server::start_positive_stream_http_server;
use nautilus_demo::init_data_engine;
use nautilus_demo::websocket_server::NegativeStreamServer;

async fn main_logic() {
    let http_address = start_positive_stream_http_server().await.unwrap();
    let websocket_server = NegativeStreamServer::setup().await;

    // Initialize data client with http and websocket streams
    let (http_stream, websocket_stream) =
        init_data_engine(http_address, websocket_server.address).await;

    // Initialize big brain actor
    let big_brain_actor = BigBrainActor::new();
    let big_brain_actor = Rc::new(UnsafeCell::new(big_brain_actor));
    register_actor(big_brain_actor);
    BigBrainActor::register_message_handlers();

    let mut runner = LiveRunner::default();
    runner.new_add_data_response_stream(http_stream);
    runner.new_message_stream(websocket_stream);
    runner.run().await;
}

pub fn main() {
    init_logger_for_testing(None).unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(main_logic());
}
