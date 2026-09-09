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

//! HTTP/1.1 exchange over the deterministic Madsim byte stream.

use bytes::Bytes;
use http::{HeaderValue, Request, Response, header::HOST};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_util::rt::TokioIo;
use url::{Position, Url};

use super::{HttpClientError, HttpRedirectPolicy};

#[derive(Clone, Debug, Default)]
pub(super) struct Client {
    redirects: HttpRedirectPolicy,
}

impl Client {
    pub(super) fn new(
        redirects: HttpRedirectPolicy,
        proxy: Option<&str>,
    ) -> Result<Self, HttpClientError> {
        if proxy.is_some() {
            return Err(HttpClientError::ClientBuildError(
                "HTTP proxies are unsupported under simulation".into(),
            ));
        }
        Ok(Self { redirects })
    }

    pub(super) async fn send(
        &self,
        mut request: Request<Full<Bytes>>,
        url: &Url,
    ) -> Result<(Response<Incoming>, Connection), HttpClientError> {
        if url.scheme() != "http" {
            return Err(HttpClientError::Error(
                "HTTP simulation supports plaintext http:// endpoints only".into(),
            ));
        }
        let host = url
            .host_str()
            .ok_or_else(|| HttpClientError::Error("missing HTTP hostname".into()))?;
        let port = url
            .port_or_known_default()
            .ok_or_else(|| HttpClientError::Error("missing HTTP port".into()))?;

        if !request.headers().contains_key(HOST) {
            let authority = &url[Position::BeforeHost..Position::AfterPort];
            request.headers_mut().insert(
                HOST,
                HeaderValue::from_str(authority)
                    .map_err(|_| HttpClientError::Error("invalid HTTP authority".into()))?,
            );
        }
        let mut headers: Vec<_> = request
            .headers()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        headers.sort_by(|(a, _), (b, _)| a.as_str().cmp(b.as_str()));
        request.headers_mut().clear();

        for (name, value) in headers {
            request.headers_mut().append(name, value);
        }
        let mut wire = request;
        *wire.uri_mut() = url[Position::BeforePath..Position::AfterQuery]
            .parse()
            .map_err(|_| HttpClientError::Error("invalid HTTP request target".into()))?;
        let stream = crate::dst::net::TcpStream::connect((host, port))
            .await
            .map_err(|_| {
                HttpClientError::TransportError("simulated HTTP connection failed".into())
            })?;
        let (mut sender, connection) = hyper::client::conn::http1::handshake(TokioIo::new(stream))
            .await
            .map_err(|_| {
                HttpClientError::TransportError("simulated HTTP handshake failed".into())
            })?;

        let connection = Connection(madsim::task::spawn(async move {
            let _ = connection.await;
        }));
        let response = sender.send_request(wire).await.map_err(|_| {
            HttpClientError::TransportError("simulated HTTP exchange failed".into())
        })?;

        if response.status().is_redirection()
            && matches!(self.redirects, HttpRedirectPolicy::Follow)
            && response.headers().contains_key(http::header::LOCATION)
        {
            return Err(HttpClientError::Error(
                "HTTP redirect following is unsupported under simulation".into(),
            ));
        }
        Ok((response, connection))
    }
}

#[derive(Debug)]
pub(super) struct Connection(madsim::task::JoinHandle<()>);

impl Drop for Connection {
    fn drop(&mut self) {
        self.0.abort();
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, time::Duration};

    use rstest::rstest;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    use crate::http::{HttpClient, HttpClientError, HttpRedirectPolicy};

    #[madsim::test]
    async fn streamed_body_retains_simulated_connection() {
        let listener = crate::dst::net::TcpListener::bind("127.0.0.1:18080")
            .await
            .unwrap();
        let peer = madsim::task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_headers(&mut stream).await;
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nabc")
                .await
                .unwrap();
            stream.flush().await.unwrap();
            madsim::time::sleep(Duration::from_secs(1)).await;
            stream.write_all(b"defgh").await.unwrap();
            stream.flush().await.unwrap();
        });

        let mut client = HttpClient::builder().timeout_secs(3).build().unwrap();
        client.client.max_response_bytes = 4;
        let mut response = client
            .get_stream("http://127.0.0.1:18080/stream".into())
            .await
            .unwrap();
        let status = response.status();
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.unwrap() {
            body.extend_from_slice(&chunk);
        }
        peer.await.unwrap();
        assert_eq!(status, http::StatusCode::OK);
        assert_eq!(body, b"abcdefgh");
    }

    #[madsim::test]
    async fn request_uses_simulated_stream_and_preserves_response() {
        let listener = crate::dst::net::TcpListener::bind("127.0.0.1:18080")
            .await
            .unwrap();
        let peer = madsim::task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let bytes = read_headers(&mut stream).await;
            stream
                .write_all(
                    b"HTTP/1.1 201 Created\r\nContent-Length: 5\r\nX-Test: exact\r\n\r\nhello",
                )
                .await
                .unwrap();
            stream.flush().await.unwrap();
            bytes
        });
        let client = HttpClient::builder()
            .headers(HashMap::from([("X-Input".into(), "declared".into())]))
            .header_keys(vec!["x-test".into()])
            .timeout_secs(2)
            .build()
            .unwrap();
        let response = client
            .get(
                "http://127.0.0.1:18080/state?product=SPOT".into(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap();
        let request = peer.await.unwrap();
        assert_eq!(request, b"GET /state?product=SPOT HTTP/1.1\r\naccept: */*\r\nhost: 127.0.0.1:18080\r\nx-input: declared\r\n\r\n");
        assert_eq!(response.status.as_u16(), 201);
        assert_eq!(
            response.headers,
            HashMap::from([("x-test".into(), "exact".into())])
        );
        assert_eq!(response.body.as_ref(), b"hello");
    }

    #[madsim::test]
    async fn post_preserves_body_and_overrides_default_headers() {
        let listener = crate::dst::net::TcpListener::bind("127.0.0.1:18080")
            .await
            .unwrap();
        let peer = madsim::task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut request = read_headers(&mut stream).await;
            let mut body = [0; 7];
            stream.read_exact(&mut body).await.unwrap();
            request.extend_from_slice(&body);
            stream
                .write_all(b"HTTP/1.1 204 No Content\r\n\r\n")
                .await
                .unwrap();
            stream.flush().await.unwrap();
            request
        });
        let client = HttpClient::builder()
            .headers(HashMap::from([("X-Input".into(), "default".into())]))
            .timeout_secs(2)
            .build()
            .unwrap();
        let response = client
            .post(
                "http://127.0.0.1:18080/order".into(),
                None,
                Some(HashMap::from([("X-Input".into(), "override".into())])),
                Some(b"payload".to_vec()),
                None,
                None,
            )
            .await
            .unwrap();
        assert_eq!(peer.await.unwrap(), b"POST /order HTTP/1.1\r\naccept: */*\r\nhost: 127.0.0.1:18080\r\nx-input: override\r\ncontent-length: 7\r\n\r\npayload");
        assert_eq!(response.status.as_u16(), 204);
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"");
    }

    #[rstest]
    #[case::timeout(false)]
    #[case::cancel(true)]
    #[madsim::test]
    async fn incomplete_body_releases_connection(#[case] cancel: bool) {
        madsim::time::timeout(Duration::from_secs(5), async {
            let listener = crate::dst::net::TcpListener::bind("127.0.0.1:18080")
                .await
                .unwrap();
            let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();

            let peer = madsim::task::spawn(async move {
                let (mut stream, _) = listener.accept().await.unwrap();
                read_headers(&mut stream).await;
                stream
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nx")
                    .await
                    .unwrap();
                stream.flush().await.unwrap();
                ready_tx.send(()).unwrap();
                let mut byte = [0];
                assert_eq!(stream.read(&mut byte).await.unwrap(), 0);
            });
            let client = HttpClient::builder().timeout_secs(20).build().unwrap();
            let request = madsim::task::spawn(async move {
                client
                    .get(
                        "http://127.0.0.1:18080/state".into(),
                        None,
                        None,
                        Some(1),
                        None,
                    )
                    .await
            });
            ready_rx.await.unwrap();

            if cancel {
                request.abort();
                assert!(request.await.unwrap_err().is_cancelled());
            } else {
                assert!(
                    matches!(request.await.unwrap(), Err(HttpClientError::TimeoutError(message))
                    if message == "simulated request deadline elapsed")
                );
            }
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }

    #[rstest]
    #[case::length(
        b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nhello",
        "HTTP response body of 5 bytes exceeds maximum of 4 bytes"
    )]
    #[case::chunked(
        b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n0\r\n\r\n",
        "HTTP response body exceeds maximum of 4 bytes"
    )]
    #[madsim::test]
    async fn response_limit_is_preserved(#[case] response: &'static [u8], #[case] message: &str) {
        let listener = crate::dst::net::TcpListener::bind("127.0.0.1:18080")
            .await
            .unwrap();
        let peer = madsim::task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_headers(&mut stream).await;
            stream.write_all(response).await.unwrap();
            stream.flush().await.unwrap();
        });
        let mut client = HttpClient::builder().timeout_secs(2).build().unwrap();
        client.client.max_response_bytes = 4;
        let error = client
            .get(
                "http://127.0.0.1:18080/state".into(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        peer.await.unwrap();
        assert!(matches!(error, HttpClientError::Error(actual) if actual == message));
    }

    #[rstest]
    #[case::reject(HttpRedirectPolicy::Reject)]
    #[case::follow(HttpRedirectPolicy::Follow)]
    #[madsim::test]
    async fn redirects_never_open_an_ambient_connection(#[case] policy: HttpRedirectPolicy) {
        let listener = crate::dst::net::TcpListener::bind("127.0.0.1:18080")
            .await
            .unwrap();
        let peer = madsim::task::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_headers(&mut stream).await;
            stream.write_all(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: https://example.com/\r\n\r\n").await.unwrap();
            stream.flush().await.unwrap();
        });
        let client = HttpClient::builder()
            .redirect_policy(policy)
            .timeout_secs(2)
            .build()
            .unwrap();
        let response = client
            .get(
                "http://127.0.0.1:18080/state".into(),
                None,
                None,
                None,
                None,
            )
            .await;
        peer.await.unwrap();
        match policy {
            HttpRedirectPolicy::Reject => assert_eq!(response.unwrap().status.as_u16(), 302),
            HttpRedirectPolicy::Follow => {
                assert!(matches!(response, Err(HttpClientError::Error(message))
                if message == "HTTP redirect following is unsupported under simulation"));
            }
        }
    }

    #[madsim::test]
    async fn tls_and_explicit_proxies_are_rejected() {
        let proxy = HttpClient::builder()
            .proxy_url("http://127.0.0.1:18080".into())
            .build()
            .unwrap_err();
        assert!(matches!(proxy, HttpClientError::ClientBuildError(message)
            if message == "HTTP proxies are unsupported under simulation"));
        let client = HttpClient::builder().timeout_secs(2).build().unwrap();
        let tls = client
            .get(
                "https://127.0.0.1:18080/state".into(),
                None,
                None,
                None,
                None,
            )
            .await
            .unwrap_err();
        assert!(matches!(tls, HttpClientError::Error(message)
            if message == "HTTP simulation supports plaintext http:// endpoints only"));
    }

    async fn read_headers(stream: &mut crate::dst::net::TcpStream) -> Vec<u8> {
        let mut bytes = Vec::new();
        while !bytes.ends_with(b"\r\n\r\n") {
            bytes.push(stream.read_u8().await.unwrap());
        }
        bytes
    }
}
