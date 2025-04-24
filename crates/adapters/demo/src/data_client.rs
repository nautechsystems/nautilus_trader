use futures::StreamExt;
use nautilus_common::actor::Actor;
use nautilus_common::actor::registry::get_actor_unchecked;
use nautilus_common::messages::data::{DataResponse, RequestData, RequestTrades};
use nautilus_common::msgbus::handler::{
    MessageHandler, ShareableMessageHandler, TypedMessageHandler,
};
use nautilus_common::msgbus::register;
use nautilus_core::UnixNanos;
use nautilus_model::data::DataType;
use nautilus_model::identifiers::Venue;
use nautilus_network::http::HttpClient;
use nautilus_network::websocket::{Consumer, WebSocketClient, WebSocketConfig};
use reqwest::Method;
use std::any::Any;
use std::net::SocketAddr;
use std::rc::Rc;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

pub struct MockNetworkDataClient {
    http_address: SocketAddr,
    http_client: HttpClient,
    websocket_address: SocketAddr,
    websocket_client: WebSocketClient,
    http_tx: tokio::sync::mpsc::UnboundedSender<DataResponse>,
}

impl MockNetworkDataClient {
    pub async fn start(
        http_address: SocketAddr,
        websocket_address: SocketAddr,
    ) -> (
        Self,
        tokio_stream::wrappers::UnboundedReceiverStream<DataResponse>,
        tokio_stream::wrappers::ReceiverStream<tokio_tungstenite::tungstenite::Message>,
    ) {
        // Create HTTP client with default settings
        let http_client = HttpClient::new(
            std::collections::HashMap::new(), // empty headers
            Vec::new(),                       // no header keys
            Vec::new(),                       // no keyed quotas
            None,                             // no default quota
            Some(5),                          // 30 second timeout
        );

        println!(
            "Started mock data client with HTTP endpoint: {:?}",
            http_address
        );
        println!("WebSocket endpoint: {:?}", websocket_address);

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let (http_tx, http_rx) = tokio::sync::mpsc::unbounded_channel();

        let config = WebSocketConfig {
            url: websocket_address.to_string(),
            headers: vec![],
            handler: Consumer::Rust(tx),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: None,
            reconnect_delay_initial_ms: None,
            reconnect_delay_max_ms: None,
            reconnect_backoff_factor: None,
            reconnect_jitter_ms: None,
        };

        let websocket_client = WebSocketClient::connect(config, None, None, None, Vec::new(), None)
            .await
            .unwrap();

        let http_stream = UnboundedReceiverStream::new(http_rx);
        let websocket_stream = ReceiverStream::new(rx);

        (
            Self {
                http_address,
                http_client,
                http_tx,
                websocket_address,
                websocket_client,
            },
            http_stream,
            websocket_stream,
        )
    }

    fn get_request(&self, req: &RequestData) {
        nautilus_common::runtime::get_runtime().block_on(async move {
            let response = self
                .http_client
                .request(
                    Method::GET,
                    format!("http://{}/get", self.http_address),
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();

            let value = String::from_utf8(response.body.to_vec())
                .unwrap()
                .parse::<i32>()
                .unwrap();
            let response = DataResponse::new(
                req.request_id,
                req.client_id,
                Venue::new("http positive stream"),
                DataType::new("positive_stream", None),
                value,
                UnixNanos::new(0),
                None,
            );
            self.http_tx.send(response).unwrap();
        });
    }

    fn skip_request(&self, req: &RequestData) {
        nautilus_common::runtime::get_runtime().block_on(async move {
            let response = self
                .http_client
                .request(
                    Method::GET,
                    format!("http://{}/skip", self.http_address),
                    None,
                    None,
                    None,
                    None,
                )
                .await
                .unwrap();

            let value = String::from_utf8(response.body.to_vec())
                .unwrap()
                .parse::<i32>()
                .unwrap();

            let response = DataResponse::new(
                req.request_id,
                req.client_id,
                Venue::new("http positive stream"),
                DataType::new("positive_stream", None),
                value,
                UnixNanos::new(0),
                None,
            );
            self.http_tx.send(response).unwrap();
        });
    }

    fn subscriber_skip_command(&self) {
        nautilus_common::runtime::get_runtime().block_on(async move {
            self.websocket_client
                .send_text("SKIP".to_string(), None)
                .await;
        });
    }

    fn subscriber_stop_command(&self) {
        nautilus_common::runtime::get_runtime().block_on(async move {
            self.websocket_client
                .send_text("STOP".to_string(), None)
                .await;
        });
    }

    pub fn register_message_handlers() {
        let handler = TypedMessageHandler::from(subscriber_skip_command_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        register("subscriber_skip_command", handler);

        let handler = TypedMessageHandler::from(get_positive_value_request_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        register("get_positive_value_request", handler);

        let handler = TypedMessageHandler::from(positive_value_skip_request_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        register("positive_value_skip_request", handler);

        let handler = TypedMessageHandler::from(subscriber_stop_command_handler);
        let handler = ShareableMessageHandler::from(Rc::new(handler) as Rc<dyn MessageHandler>);
        register("subscriber_stop_command", handler);
    }
}

impl Actor for MockNetworkDataClient {
    fn id(&self) -> Ustr {
        Ustr::from("mock_network_data_client")
    }

    fn handle(&mut self, msg: &dyn Any) {
        todo!()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub fn subscriber_skip_command_handler(req: &()) {
    println!("DataClient: Received subscriber skip command");
    let actor_id = Ustr::from("mock_network_data_client");
    let data_client = get_actor_unchecked::<MockNetworkDataClient>(&actor_id);
    data_client.subscriber_skip_command();
}

pub fn subscriber_stop_command_handler(req: &()) {
    println!("DataClient: Received subscriber stop command");
    let actor_id = Ustr::from("mock_network_data_client");
    let data_client = get_actor_unchecked::<MockNetworkDataClient>(&actor_id);
    data_client.subscriber_stop_command();
}

pub fn get_positive_value_request_handler(req: &RequestData) {
    println!("DataClient: Received get positive value request");
    let actor_id = Ustr::from("mock_network_data_client");
    let data_client = get_actor_unchecked::<MockNetworkDataClient>(&actor_id);
    data_client.get_request(req);
}

pub fn positive_value_skip_request_handler(req: &RequestData) {
    println!("DataClient: Received positive value skip request");
    let actor_id = Ustr::from("mock_network_data_client");
    let data_client = get_actor_unchecked::<MockNetworkDataClient>(&actor_id);
    data_client.skip_request(req);
}
