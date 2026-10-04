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

//! Local HTTP redirect assertions for client constructor tests.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// Asserts that a GET request returns the original 307 without contacting its redirect target.
///
/// `send` receives a loopback URL and returns the response status code. It must issue one
/// GET request through the client under test without overriding the client's redirect policy.
///
/// # Panics
///
/// - Binding a listener or reading its local address fails.
/// - `send` panics or takes more than five seconds.
/// - The response status is not 307 or the destination receives a request.
pub async fn assert_http_redirect_rejected<F, Fut>(send: F)
where
    F: FnOnce(String) -> Fut,
    Fut: Future<Output = u16>,
{
    let origin = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = origin.local_addr().unwrap();
    let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = destination.local_addr().unwrap();
    let destination_requests = Arc::new(AtomicUsize::new(0));
    let requests = destination_requests.clone();

    let destination_task = tokio::spawn(async move {
        let (mut stream, _) = destination.accept().await.unwrap();
        read_request_headers(&mut stream).await;
        requests.fetch_add(1, Ordering::SeqCst);
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n")
            .await
            .unwrap();
    });

    let origin_task = tokio::spawn(async move {
        let (mut stream, _) = origin.accept().await.unwrap();
        read_request_headers(&mut stream).await;
        stream.write_all(format!("HTTP/1.1 307 Temporary Redirect\r\nContent-Length: 0\r\nConnection: close\r\nLocation: http://{target}/destination\r\n\r\n").as_bytes()).await.unwrap();
    });
    let result = tokio::time::timeout(
        Duration::from_secs(5),
        send(format!("http://{addr}/origin")),
    )
    .await;
    origin_task.abort();
    destination_task.abort();

    assert_eq!(result.unwrap(), 307);
    assert_eq!(destination_requests.load(Ordering::SeqCst), 0);
}

async fn read_request_headers(stream: &mut tokio::net::TcpStream) {
    let mut headers = Vec::new();
    while !headers.ends_with(b"\r\n\r\n") {
        let mut byte = [0];
        stream.read_exact(&mut byte).await.unwrap();
        headers.push(byte[0]);
    }
}
