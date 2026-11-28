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

//! Test stubs shared by the data and execution tests.

use futures_util::Stream;
use ibapi::{Error, subscriptions::SubscriptionItem};
use nautilus_core::UnixNanos;
use nautilus_model::{
    identifiers::{InstrumentId, Symbol},
    instruments::{CurrencyPair, InstrumentAny},
    types::{Currency, Price, Quantity},
};
use serde::Deserialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Channel-backed stand-in for an ibapi subscription; items arrive as data.
pub(crate) struct ChannelSubscription<T>(tokio::sync::mpsc::UnboundedReceiver<Result<T, Error>>);

impl<T> ChannelSubscription<T> {
    pub(crate) const fn new(
        receiver: tokio::sync::mpsc::UnboundedReceiver<Result<T, Error>>,
    ) -> Self {
        Self(receiver)
    }

    pub(crate) fn close(&mut self) {
        self.0.close();
    }
}

impl<T> Stream for ChannelSubscription<T> {
    type Item = Result<SubscriptionItem<T>, Error>;

    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        self.0
            .poll_recv(cx)
            .map(|item| item.map(|result| result.map(SubscriptionItem::Data)))
    }
}

/// Builds a spot currency pair for tests, FX or crypto by its currencies.
pub(crate) fn currency_pair(
    base: &str,
    quote: &str,
    price_precision: u8,
    price_increment: &str,
) -> InstrumentAny {
    let instrument_id = InstrumentId::from(format!("{base}/{quote}.IDEALPRO").as_str());
    let pair = CurrencyPair::builder()
        .instrument_id(instrument_id)
        .raw_symbol(Symbol::from(format!("{base}.{quote}").as_str()))
        .base_currency(Currency::from(base))
        .quote_currency(Currency::from(quote))
        .price_precision(price_precision)
        .size_precision(0)
        .price_increment(Price::from(price_increment))
        .size_increment(Quantity::from("1"))
        .ts_event(UnixNanos::default())
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    InstrumentAny::from(pair)
}

#[derive(Deserialize)]
pub(crate) struct IbExchange {
    pub(crate) request: i32,
    pub(crate) responses: Vec<Vec<u8>>,
}

pub(crate) async fn with_ib_replay<R>(
    exchanges: Vec<IbExchange>,
    run: impl AsyncFnOnce(std::sync::Arc<ibapi::Client>) -> R,
) -> R {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    let server = serve_ib_exchanges(listener, exchanges, done_rx);
    let request = async {
        let client = std::sync::Arc::new(
            ibapi::Client::connect(&address.to_string(), 7)
                .await
                .unwrap(),
        );
        let result = run(std::sync::Arc::clone(&client)).await;
        client.disconnect().await;
        done_tx.send(()).unwrap();
        result
    };

    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let (result, ()) = tokio::join!(request, server);
        result
    })
    .await
    .unwrap()
}

pub(crate) async fn serve_ib_exchanges(
    listener: tokio::net::TcpListener,
    exchanges: Vec<IbExchange>,
    done: tokio::sync::oneshot::Receiver<()>,
) {
    let (mut socket, _) = listener.accept().await.unwrap();
    let mut magic = [0_u8; 4];
    socket.read_exact(&mut magic).await.unwrap();
    assert_eq!(magic, *b"API\0");
    let _version = read_ib_frame(&mut socket).await;
    let handshake = b"213\x0020260924 12:03:00 UTC\0";
    socket.write_u32(handshake.len() as u32).await.unwrap();
    socket.write_all(handshake).await.unwrap();
    for exchange in exchanges {
        let request = read_ib_frame(&mut socket).await;
        let message_id = i32::from_be_bytes(request[..4].try_into().unwrap());
        assert_eq!(message_id, exchange.request);

        for response in exchange.responses {
            socket.write_all(&response).await.unwrap();
        }
    }

    done.await.unwrap();
}

async fn read_ib_frame(socket: &mut tokio::net::TcpStream) -> Vec<u8> {
    let length = socket.read_u32().await.unwrap();
    let mut frame = vec![0; length as usize];
    socket.read_exact(&mut frame).await.unwrap();
    frame
}
