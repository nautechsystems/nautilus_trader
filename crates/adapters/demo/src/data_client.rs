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

use std::{net::SocketAddr, pin::Pin, sync::Arc};

use futures::{Stream, StreamExt};
use nautilus_common::{
    messages::data::{
        CustomDataResponse, DataResponse, RequestCustomData, SubscribeCustomData,
        UnsubscribeCustomData,
    },
    runtime,
};
use nautilus_core::UnixNanos;
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::DataType,
    identifiers::{ClientId, Venue},
};
use nautilus_network::{
    http::HttpClient,
    websocket::{Consumer, WebSocketClient, WebSocketConfig},
};
use reqwest::Method;
use tokio_stream::wrappers::{ReceiverStream, UnboundedReceiverStream};
use tokio_tungstenite::tungstenite::Message;

pub struct MockDataClient {
    http_address: SocketAddr,
    http_client: HttpClient,
    websocket_client: Arc<WebSocketClient>,
    http_tx: tokio::sync::mpsc::UnboundedSender<DataResponse>,
}

impl MockDataClient {
    pub async fn start(
        http_address: SocketAddr,
        websocket_address: SocketAddr,
    ) -> (
        Self,
        Pin<Box<dyn Stream<Item = DataResponse>>>,
        Pin<Box<dyn Stream<Item = i32>>>,
    ) {
        // Create HTTP client with default settings
        let http_client = HttpClient::new(
            std::collections::HashMap::new(), // empty headers
            Vec::new(),                       // no header keys
            Vec::new(),                       // no keyed quotas
            None,                             // no default quota
            Some(5),                          // 30 second timeout
        );

        println!("Started mock data client with HTTP endpoint: {http_address:?}");
        println!("WebSocket endpoint: {websocket_address:?}");

        let (tx, rx) = tokio::sync::mpsc::channel(100);
        let (http_tx, http_rx) = tokio::sync::mpsc::unbounded_channel();

        let config = WebSocketConfig {
            url: format!("ws://{websocket_address}"),
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

        let websocket_stream = ReceiverStream::new(rx).map(|message| match message {
            Message::Text(text) => text.parse::<i32>().unwrap(),
            _ => unreachable!("Expected Message::Text"),
        });

        (
            Self {
                http_address,
                http_client,
                http_tx,
                websocket_client: Arc::new(websocket_client),
            },
            Box::pin(http_stream),
            Box::pin(websocket_stream),
        )
    }

    fn get_request(&self, req: &RequestCustomData) {
        let req = req.clone();
        let http_client = self.http_client.clone();
        let http_tx = self.http_tx.clone();
        let http_address = self.http_address;
        runtime::get_runtime().spawn(async move {
            let response = http_client
                .request(
                    Method::GET,
                    format!("http://{http_address}/get"),
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
            println!("Received positive value: {value}");

            let response = DataResponse::Data(CustomDataResponse::new(
                req.request_id,
                req.client_id,
                Some(Venue::new("http positive stream")),
                DataType::new("positive_stream", None),
                Arc::new(value),
                UnixNanos::new(0),
                None,
            ));
            http_tx.send(response).unwrap();
        });
    }

    fn skip_request(&self, req: &RequestCustomData) {
        let req = req.clone();
        let http_client = self.http_client.clone();
        let http_tx = self.http_tx.clone();
        let http_address = self.http_address;
        runtime::get_runtime().spawn(async move {
            let response = http_client
                .request(
                    Method::GET,
                    format!("http://{http_address}/skip"),
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
            println!("Received positive value: {value}");

            let response = DataResponse::Data(CustomDataResponse::new(
                req.request_id,
                req.client_id,
                Some(Venue::new("http positive stream")),
                DataType::new("positive_stream", None),
                Arc::new(value),
                UnixNanos::new(0),
                None,
            ));
            http_tx.send(response).unwrap();
        });
    }
}

#[async_trait::async_trait]
impl DataClient for MockDataClient {
    fn client_id(&self) -> nautilus_model::identifiers::ClientId {
        ClientId::new("mock_data_client")
    }

    fn request_data(&self, request: &RequestCustomData) -> anyhow::Result<()> {
        if request.data_type.type_name() == "get" {
            println!("Received get data request");
            self.get_request(request);
        } else if request.data_type.type_name() == "skip" {
            println!("Received skip data request");
            self.skip_request(request);
        }

        Ok(())
    }

    fn subscribe(&mut self, _cmd: &SubscribeCustomData) -> anyhow::Result<()> {
        println!("Received subscribe command");
        let websocket_client = self.websocket_client.clone();
        runtime::get_runtime().spawn(async move {
            if let Err(err) = websocket_client.send_text("SKIP".to_string(), None).await {
                tracing::error!("Error sending SKIP message: {err:?}");
            }
        });
        Ok(())
    }

    fn unsubscribe(&mut self, _cmd: &UnsubscribeCustomData) -> anyhow::Result<()> {
        println!("Received unsubscribe command");
        let websocket_client = self.websocket_client.clone();
        runtime::get_runtime().spawn(async move {
            if let Err(err) = websocket_client.send_text("STOP".to_string(), None).await {
                tracing::error!("Error sending STOP message: {err:?}");
            }
        });
        Ok(())
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn reset(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn dispose(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn connect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    async fn disconnect(&self) -> anyhow::Result<()> {
        Ok(())
    }

    fn is_connected(&self) -> bool {
        true
    }

    fn is_disconnected(&self) -> bool {
        false
    }
}
