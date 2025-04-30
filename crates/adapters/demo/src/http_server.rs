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

use std::net::{SocketAddr, TcpListener};

use axum::{Router, routing::get, serve};

fn get_unique_port() -> u16 {
    // Create a temporary TcpListener to get an available port
    let listener = TcpListener::bind("127.0.0.1:0").expect("Failed to bind temporary TcpListener");
    let port = listener.local_addr().unwrap().port();

    // Close the listener to free up the port
    drop(listener);

    port
}

pub async fn start_positive_stream_http_server()
-> Result<SocketAddr, Box<dyn std::error::Error + Send + Sync>> {
    let port = get_unique_port();
    let listener = tokio::net::TcpListener::bind(format!("127.0.0.1:{port}"))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        serve(listener, create_positive_stream_router())
            .await
            .unwrap();
    });

    Ok(addr)
}

fn create_positive_stream_router() -> Router {
    // Create a counter state that will be shared across requests
    let counter = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));

    // Clone the counter for the handler
    let counter_clone = counter.clone();
    let counter_clone_2 = counter;

    Router::new()
        .route(
            "/get",
            get(async move || {
                // Increment the counter and return the new value
                let value = counter_clone.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                format!("{value}")
            }),
        )
        .route(
            "/skip",
            get(async move || {
                // Increment the counter and return the new value
                let value = counter_clone_2.fetch_add(5, std::sync::atomic::Ordering::SeqCst);
                format!("{value}")
            }),
        )
}
