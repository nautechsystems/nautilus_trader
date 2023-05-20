// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use std::convert::Infallible;
use std::net::SocketAddr;

use criterion::{criterion_group, Criterion};
use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Request, Response, Server};
use tokio::runtime::Runtime;

async fn handle(_req: Request<Body>) -> Result<Response<Body>, Infallible> {
    Ok(Response::new(Body::from("Hello World")))
}

fn start_server(rt: &Runtime, addr: SocketAddr) {
    let make_service = make_service_fn(|_conn| async { Ok::<_, Infallible>(service_fn(handle)) });
    let server = Server::bind(&addr).serve(make_service);

    rt.spawn(async move {
        if let Err(e) = server.await {
            eprintln!("server error: {}", e);
        }
    });
}

fn server_benchmark(c: &mut Criterion) {
    let rt = Runtime::new().unwrap();
    let addr = SocketAddr::from(([127, 0, 0, 1], 3000));
    start_server(&rt, addr);

    // Here we would generate load on the server and measure its response.
    // We're just sleeping for demonstration purposes.
    c.bench_function("server", |b| {
        b.iter(|| std::thread::sleep(std::time::Duration::from_millis(10)))
    });
}

criterion_group!(benches, server_benchmark);
