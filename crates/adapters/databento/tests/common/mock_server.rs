// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Mock Databento LSG (Live Streaming Gateway) server for integration testing.
//!
//! Modeled after databento's own `MockGateway` + `Fixture` pattern
//! in `databento-0.44.0/src/live/client.rs`.

use std::fmt::Debug;

use databento::dbn::{MetadataBuilder, SType, encode::dbn::AsyncMetadataEncoder, record::HasRType};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream},
};

pub enum MockEvent {
    Authenticate,
    AuthenticateReject(String),
    ExpectSubscription,
    Start,
    SendRecord(Box<dyn AsRef<[u8]> + Send>),
    Disconnect,
    Exit,
}

impl Debug for MockEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Authenticate => write!(f, "Authenticate"),
            Self::AuthenticateReject(msg) => write!(f, "AuthenticateReject({msg:?})"),
            Self::ExpectSubscription => write!(f, "ExpectSubscription"),
            Self::Start => write!(f, "Start"),
            Self::SendRecord(_) => write!(f, "SendRecord"),
            Self::Disconnect => write!(f, "Disconnect"),
            Self::Exit => write!(f, "Exit"),
        }
    }
}

struct MockGateway {
    dataset: String,
    listener: TcpListener,
    stream: Option<BufReader<TcpStream>>,
}

impl MockGateway {
    async fn new(dataset: String) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        Self {
            dataset,
            listener,
            stream: None,
        }
    }

    fn port(&self) -> u16 {
        self.listener.local_addr().unwrap().port()
    }

    async fn accept(&mut self) {
        let stream = self.listener.accept().await.unwrap().0;
        stream.set_nodelay(true).unwrap();
        self.stream = Some(BufReader::new(stream));
    }

    async fn authenticate(&mut self) {
        self.accept().await;
        self.send_text("lsg-test\n").await;
        self.send_text("cram=test-challenge\n").await;
        let _auth_line = self.read_line().await;
        self.send_text("success=1|session_id=test-session\n").await;
    }

    async fn authenticate_reject(&mut self, error: &str) {
        self.accept().await;
        self.send_text("lsg-test\n").await;
        self.send_text("cram=test-challenge\n").await;
        let _auth_line = self.read_line().await;
        self.send_text(&format!("success=0|error={error}\n")).await;
    }

    async fn expect_subscription(&mut self) {
        let _sub_line = self.read_line().await;
    }

    async fn start(&mut self) {
        let start_line = self.read_line().await;
        assert_eq!(start_line.trim(), "start_session");

        let dataset = self.dataset.clone();
        let stream = self.stream();
        let metadata = MetadataBuilder::new()
            .dataset(dataset)
            .start(1_000_000_000u64)
            .schema(None)
            .stype_in(None)
            .stype_out(SType::InstrumentId)
            .build();
        let mut encoder = AsyncMetadataEncoder::new(stream);
        encoder.encode(&metadata).await.unwrap();
    }

    async fn send_record(&mut self, record: Box<dyn AsRef<[u8]> + Send>) {
        let bytes = (*record).as_ref();
        let half = bytes.len() / 2;
        self.stream().write_all(&bytes[..half]).await.unwrap();
        self.stream().flush().await.unwrap();
        self.stream().write_all(&bytes[half..]).await.unwrap();
        self.stream().flush().await.unwrap();
    }

    async fn disconnect(&mut self) {
        if let Some(stream) = self.stream.as_mut() {
            stream.shutdown().await.unwrap();
        }
        self.stream = None;
    }

    async fn send_text(&mut self, text: &str) {
        self.stream().write_all(text.as_bytes()).await.unwrap();
    }

    async fn read_line(&mut self) -> String {
        let mut line = String::new();
        self.stream().read_line(&mut line).await.unwrap();
        line
    }

    fn stream(&mut self) -> &mut BufReader<TcpStream> {
        self.stream.as_mut().expect("no active connection")
    }
}

pub struct MockLsgServer {
    tx: tokio::sync::mpsc::UnboundedSender<MockEvent>,
    port: u16,
    task: tokio::task::JoinHandle<()>,
}

impl MockLsgServer {
    pub async fn new(dataset: &str) -> Self {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut gateway = MockGateway::new(dataset.to_string()).await;
        let port = gateway.port();

        let task = tokio::task::spawn(async move {
            loop {
                match rx.recv().await {
                    Some(MockEvent::Authenticate) => gateway.authenticate().await,
                    Some(MockEvent::AuthenticateReject(error)) => {
                        gateway.authenticate_reject(&error).await;
                    }
                    Some(MockEvent::ExpectSubscription) => gateway.expect_subscription().await,
                    Some(MockEvent::Start) => gateway.start().await,
                    Some(MockEvent::SendRecord(record)) => gateway.send_record(record).await,
                    Some(MockEvent::Disconnect) => gateway.disconnect().await,
                    Some(MockEvent::Exit) | None => break,
                }
            }
        });

        Self { tx, port, task }
    }

    pub fn addr(&self) -> String {
        format!("127.0.0.1:{}", self.port)
    }

    pub fn authenticate(&self) {
        self.tx.send(MockEvent::Authenticate).unwrap();
    }

    pub fn authenticate_reject(&self, error: &str) {
        self.tx
            .send(MockEvent::AuthenticateReject(error.to_string()))
            .unwrap();
    }

    pub fn expect_subscription(&self) {
        self.tx.send(MockEvent::ExpectSubscription).unwrap();
    }

    pub fn start(&self) {
        self.tx.send(MockEvent::Start).unwrap();
    }

    pub fn send_record<R>(&self, record: R)
    where
        R: HasRType + AsRef<[u8]> + Clone + Send + 'static,
    {
        self.tx
            .send(MockEvent::SendRecord(Box::new(record)))
            .unwrap();
    }

    pub fn disconnect(&self) {
        self.tx.send(MockEvent::Disconnect).unwrap();
    }

    pub async fn stop(self) {
        self.tx.send(MockEvent::Exit).unwrap();
        self.task.await.unwrap();
    }
}
