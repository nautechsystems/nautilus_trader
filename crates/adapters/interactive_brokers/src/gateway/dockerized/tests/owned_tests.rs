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

use std::{collections::VecDeque, time::Duration};

use bollard::{ClientVersion, Docker, errors::Error as DockerError};
use nautilus_core::string::secret::SecretString;
#[cfg(feature = "python")]
use pyo3::{
    exceptions::{PyAttributeError, PyRuntimeError, PyTypeError},
    prelude::*,
};
use rstest::rstest;
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::TcpListener,
};

use super::DockerizedIBGateway;
#[cfg(feature = "python")]
use super::OwnedIBGateway;
use crate::config::{DockerizedIBGatewayConfig, TradingMode};

#[tokio::test]
async fn test_owned_create_name_conflict() {
    let mut server = DockerServer::new(vec![Reply::json(
        409,
        json!({"message": "Conflict: container name is already in use"}),
    )])
    .await;
    let gateway = server.gateway();

    let e = gateway.create_owned().await.unwrap_err();

    assert!(matches!(
        e.downcast_ref::<DockerError>(),
        Some(DockerError::DockerResponseServerError {
            status_code: 409,
            ..
        })
    ));
    server.assert_requests(&[("POST", "/containers/create")]);
}

#[tokio::test]
async fn test_owned_create_returns_handle_before_start() {
    let mut server = DockerServer::new(vec![Reply::json(
        201,
        json!({"Id": "owned-a", "Warnings": []}),
    )])
    .await;
    let gateway = server.gateway();

    let owned = gateway.create_owned().await.unwrap();

    assert_eq!(owned.container_id(), "owned-a");
    assert_eq!(owned.host(), "127.0.0.1");
    assert_eq!(owned.port(), 4002);
    server.assert_requests(&[("POST", "/containers/create")]);
    assert!(
        server.requests[0]
            .1
            .contains("name=nautilus-ib-gateway-paper")
    );
}

#[tokio::test]
async fn test_owned_create_rejects_empty_container_id() {
    let mut server =
        DockerServer::new(vec![Reply::json(201, json!({"Id": "", "Warnings": []}))]).await;

    let e = server.gateway().create_owned().await.unwrap_err();

    assert!(e.to_string().contains("empty container ID"));
    server.assert_requests(&[("POST", "/containers/create")]);
}

#[tokio::test]
async fn test_owned_start_does_not_recreate_removed_container() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::json(404, json!({"message": "No such container: owned-a"})),
    ])
    .await;
    let owned = server.gateway().create_owned().await.unwrap();

    assert!(owned.start(Some(1)).await.is_err());
    assert_eq!(owned.container_id(), "owned-a");
    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/start"),
    ]);
}

#[tokio::test]
async fn test_owned_stop_failure_does_not_remove_and_can_retry() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::json(500, json!({"message": "Container stop failed"})),
        Reply::empty(204),
        Reply::empty(204),
    ])
    .await;
    let owned = server.gateway().create_owned().await.unwrap();

    assert!(owned.stop().await.is_err());
    owned.stop().await.unwrap();

    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/stop"),
        ("POST", "/containers/owned-a/stop"),
        ("DELETE", "/containers/owned-a"),
    ]);
}

#[rstest]
#[case::running(204, 204)]
#[case::already_stopped(304, 204)]
#[case::removed_during_cleanup(204, 404)]
#[tokio::test]
async fn test_owned_clone_cleanup(#[case] stop_status: u16, #[case] remove_status: u16) {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::empty(stop_status),
        Reply::empty(remove_status),
        Reply::json(404, json!({"message": "No such container: owned-a"})),
    ])
    .await;
    let owned = server.gateway().create_owned().await.unwrap();
    let cloned = owned.clone();

    cloned.stop().await.unwrap();
    owned.stop().await.unwrap();

    assert_eq!(cloned.container_id(), "owned-a");
    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/stop"),
        ("DELETE", "/containers/owned-a"),
        ("POST", "/containers/owned-a/stop"),
    ]);
}

#[tokio::test]
async fn test_owned_cleanup_after_same_name_replacement() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::empty(204),
        Reply::json(201, json!({"Id": "owned-b", "Warnings": []})),
        Reply::json(404, json!({"message": "No such container: owned-a"})),
        Reply::json(404, json!({"message": "No such container: owned-a"})),
    ])
    .await;
    let gateway = server.gateway();
    let old = gateway.create_owned().await.unwrap();

    // An external owner removes A, then Docker assigns the same name to B.
    gateway
        .docker
        .remove_container("owned-a", None)
        .await
        .unwrap();
    let replacement = gateway.create_owned().await.unwrap();
    old.stop().await.unwrap();
    old.clone().stop().await.unwrap();

    assert_eq!(replacement.container_id(), "owned-b");
    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("DELETE", "/containers/owned-a"),
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/stop"),
        ("POST", "/containers/owned-a/stop"),
    ]);
    assert_eq!(server.requests[0].1, server.requests[2].1);
}

#[tokio::test]
async fn test_owned_cleanup_after_start_failure() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::json(500, json!({"message": "Container startup failed"})),
        Reply::empty(204),
        Reply::empty(204),
    ])
    .await;
    let owned = server.gateway().create_owned().await.unwrap();

    let e = owned.start(Some(1)).await.unwrap_err();
    owned.stop().await.unwrap();

    assert!(matches!(
        e.downcast_ref::<DockerError>(),
        Some(DockerError::DockerResponseServerError {
            status_code: 500,
            ..
        })
    ));
    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/start"),
        ("POST", "/containers/owned-a/stop"),
        ("DELETE", "/containers/owned-a"),
    ]);
}

#[tokio::test]
async fn test_owned_start_already_running_waits_for_readiness() {
    let message = b"2026-10-31T00:00:01Z Login has completed\n";
    let mut logs = vec![1, 0, 0, 0];
    logs.extend_from_slice(&u32::try_from(message.len()).unwrap().to_be_bytes());
    logs.extend_from_slice(message);

    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::empty(304),
        Reply::json(200, json!({"State": {"StartedAt": "2026-10-31T00:00:00Z"}})),
        Reply::Response(200, logs),
    ])
    .await;
    let owned = server.gateway().create_owned().await.unwrap();

    owned.start(Some(1)).await.unwrap();

    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/start"),
        ("GET", "/containers/owned-a/json"),
        ("GET", "/containers/owned-a/logs"),
    ]);
}

#[tokio::test]
async fn test_owned_cleanup_after_readiness_wait_cancellation() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::empty(204),
        Reply::Pending,
        Reply::empty(204),
        Reply::empty(204),
    ])
    .await;
    let owned = server.gateway().create_owned().await.unwrap();

    {
        let startup = owned.start(Some(30));
        tokio::pin!(startup);

        tokio::select! {
            biased;
            result = &mut startup => panic!("Startup completed before cancellation: {result:?}"),
            () = server.wait_for_requests(3) => {}
        }
    }

    owned.stop().await.unwrap();

    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/start"),
        ("GET", "/containers/owned-a/json"),
        ("POST", "/containers/owned-a/stop"),
        ("DELETE", "/containers/owned-a"),
    ]);
}

#[cfg(feature = "python")]
#[tokio::test]
async fn test_python_owned_blocking_cleanup_after_start_failure() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::json(500, json!({"message": "Container startup failed"})),
        Reply::empty(204),
        Reply::empty(204),
    ])
    .await;
    let gateway = server.gateway();

    tokio::task::spawn_blocking(move || {
        Python::initialize();
        Python::attach(|py| {
            let gateway = Py::new(py, gateway).unwrap();
            let owned = gateway
                .bind(py)
                .call_method0("create_owned_blocking")
                .unwrap();
            assert_python_owned_contract(py, &owned);

            let e = owned.call_method1("start_blocking", (1,)).unwrap_err();
            assert!(e.is_instance_of::<PyRuntimeError>(py));
            assert_eq!(
                owned
                    .getattr("container_id")
                    .unwrap()
                    .extract::<String>()
                    .unwrap(),
                "owned-a",
            );
            owned.call_method0("stop_blocking").unwrap();
        });
    })
    .await
    .unwrap();

    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/start"),
        ("POST", "/containers/owned-a/stop"),
        ("DELETE", "/containers/owned-a"),
    ]);
}

#[cfg(feature = "python")]
#[tokio::test]
async fn test_python_owned_async_cleanup_after_cancellation() {
    let mut server = DockerServer::new(vec![
        Reply::json(201, json!({"Id": "owned-a", "Warnings": []})),
        Reply::empty(204),
        Reply::Pending,
        Reply::empty(204),
        Reply::empty(204),
    ])
    .await;
    let gateway = server.gateway();
    let (sender, receiver) = tokio::sync::oneshot::channel();

    let python = tokio::task::spawn_blocking(move || {
        Python::initialize();
        Python::attach(|py| {
            let module = PyModule::from_code(
                py,
                c"import asyncio

async def exercise(gateway):
    owned = await gateway.create_owned()
    assert owned.container_id == 'owned-a'
    try:
        await owned.start(30)
    except asyncio.CancelledError:
        pass
    else:
        raise AssertionError('Startup completed before cancellation')
    assert owned.container_id == 'owned-a'
    await owned.stop()
    return owned

async def bounded_result(task):
    return await asyncio.wait_for(asyncio.shield(task), 5)
",
                c"owned_gateway_test.py",
                c"owned_gateway_test",
            )
            .unwrap();
            let event_loop = py
                .import("asyncio")
                .unwrap()
                .call_method0("new_event_loop")
                .unwrap();
            let gateway = Py::new(py, gateway).unwrap();
            let exercise = module
                .getattr("exercise")
                .unwrap()
                .call1((gateway,))
                .unwrap();
            let task = event_loop.call_method1("create_task", (exercise,)).unwrap();
            sender
                .send((event_loop.clone().unbind(), task.clone().unbind()))
                .unwrap();
            let completion = module
                .getattr("bounded_result")
                .unwrap()
                .call1((&task,))
                .unwrap();
            let result = event_loop.call_method1("run_until_complete", (completion,));
            event_loop.call_method0("close").unwrap();
            assert_python_owned_contract(py, &result.unwrap());
        });
    });

    let (event_loop, task) = receiver.await.unwrap();
    server.wait_for_requests(3).await;
    Python::attach(|py| {
        let cancel = task.bind(py).getattr("cancel").unwrap();
        event_loop
            .bind(py)
            .call_method1("call_soon_threadsafe", (cancel,))
            .unwrap();
    });
    python.await.unwrap();

    server.assert_requests(&[
        ("POST", "/containers/create"),
        ("POST", "/containers/owned-a/start"),
        ("GET", "/containers/owned-a/json"),
        ("POST", "/containers/owned-a/stop"),
        ("DELETE", "/containers/owned-a"),
    ]);
}

#[cfg(feature = "python")]
fn assert_python_owned_contract(py: Python<'_>, owned: &Bound<'_, PyAny>) {
    let class = py.get_type::<OwnedIBGateway>();
    assert_eq!(
        class
            .getattr("__module__")
            .unwrap()
            .extract::<String>()
            .unwrap(),
        "nautilus_trader.adapters.interactive_brokers",
    );
    assert!(class.call0().unwrap_err().is_instance_of::<PyTypeError>(py));
    assert!(
        class
            .call1(("foreign-id",))
            .unwrap_err()
            .is_instance_of::<PyTypeError>(py),
    );
    assert!(
        owned
            .setattr("container_id", "foreign-id")
            .unwrap_err()
            .is_instance_of::<PyAttributeError>(py),
    );
    assert_eq!(
        owned
            .getattr("container_id")
            .unwrap()
            .extract::<String>()
            .unwrap(),
        "owned-a",
    );
    let cloned = owned.extract::<OwnedIBGateway>().unwrap();
    assert_eq!(cloned.container_id(), "owned-a");
}

struct DockerServer {
    docker: Docker,
    requests: Vec<(String, String)>,
    incoming: tokio::sync::mpsc::UnboundedReceiver<(String, String)>,
    task: tokio::task::JoinHandle<()>,
}

impl DockerServer {
    async fn new(replies: Vec<Reply>) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let docker = Docker::connect_with_http(
            &format!("http://{address}"),
            5,
            &ClientVersion {
                major_version: 1,
                minor_version: 53,
            },
        )
        .unwrap();
        let (sender, incoming) = tokio::sync::mpsc::unbounded_channel();
        let mut replies = VecDeque::from(replies);

        let task = tokio::spawn(async move {
            // Held replies keep readiness pending while the server accepts cleanup requests.
            let mut held_connections = Vec::new();

            loop {
                let (socket, _) = listener.accept().await.unwrap();
                let mut socket = BufReader::new(socket);
                let mut line = String::new();
                socket.read_line(&mut line).await.unwrap();
                let mut words = line.split_whitespace();
                let method = words.next().unwrap().to_owned();
                let target = words
                    .next()
                    .unwrap()
                    .strip_prefix("/v1.53")
                    .unwrap()
                    .to_owned();
                let mut content_length = 0;

                loop {
                    line.clear();
                    socket.read_line(&mut line).await.unwrap();

                    if line == "\r\n" {
                        break;
                    }

                    let (name, value) = line.split_once(':').unwrap();

                    if name.eq_ignore_ascii_case("content-length") {
                        content_length = value.trim().parse::<usize>().unwrap();
                    }
                }

                let mut body = vec![0; content_length];
                socket.read_exact(&mut body).await.unwrap();
                let _ = sender.send((method, target));

                let reply = replies.pop_front().unwrap_or_else(|| {
                    Reply::json(500, json!({"message": "Unexpected Docker request"}))
                });

                match reply {
                    Reply::Pending => held_connections.push(socket),
                    Reply::Response(status, body) => {
                        let header = format!(
                            "HTTP/1.1 {status} Test\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                            body.len()
                        );
                        socket.write_all(header.as_bytes()).await.unwrap();
                        socket.write_all(&body).await.unwrap();
                        socket.shutdown().await.unwrap();
                    }
                }
            }
        });

        Self {
            docker,
            requests: Vec::new(),
            incoming,
            task,
        }
    }

    fn gateway(&self) -> DockerizedIBGateway {
        // Build the real gateway with a real HTTP transport, without a local Docker daemon.
        DockerizedIBGateway {
            config: DockerizedIBGatewayConfig::builder()
                .trading_mode(TradingMode::Paper)
                .build(),
            docker: self.docker.clone(),
            username: SecretString::from("fixture-user"),
            password: SecretString::from("fixture-password"),
            host: "127.0.0.1".to_owned(),
            port: 4002,
            container_name: "nautilus-ib-gateway-paper".to_owned(),
        }
    }

    async fn wait_for_requests(&mut self, count: usize) {
        tokio::time::timeout(Duration::from_secs(5), async {
            while self.requests.len() < count {
                self.requests.push(self.incoming.recv().await.unwrap());
            }
        })
        .await
        .expect("Docker request was not observed");
    }

    fn assert_requests(&mut self, expected: &[(&str, &str)]) {
        while let Ok(request) = self.incoming.try_recv() {
            self.requests.push(request);
        }

        let actual: Vec<_> = self
            .requests
            .iter()
            .map(|(method, target)| (method.as_str(), target.split('?').next().unwrap()))
            .collect();
        assert_eq!(actual, expected);
    }
}

impl Drop for DockerServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

enum Reply {
    Response(u16, Vec<u8>),
    Pending,
}

impl Reply {
    fn json(status: u16, body: Value) -> Self {
        Self::Response(status, serde_json::to_vec(&body).unwrap())
    }

    fn empty(status: u16) -> Self {
        Self::Response(status, Vec::new())
    }
}
