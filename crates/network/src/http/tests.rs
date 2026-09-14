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

//! HTTP wire behavior and connection lifecycle regressions.

use std::{collections::HashMap, time::Duration};

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use bytes::Bytes;
use http::Method;
use rstest::rstest;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use super::{HttpClient, HttpClientError, HttpResponse};

#[tokio::test]
async fn pooled_requests_consume_complete_bodies() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        for _ in 0..3 {
            let request = read_headers(&mut stream).await;
            assert!(request.starts_with("GET /pool?asset=BTC HTTP/1.1\r\n"));
            stream.write_all(b"HTTP/1.1 206 Partial Content\r\nTransfer-Encoding: chunked\r\nX-Receipt: receipt-53\r\n\r\n3\r\nabc\r\n4\r\ndefg\r\n0\r\n\r\n").await.unwrap();
        }
    });
    let client = HttpClient::builder()
        .header_keys(vec!["x-receipt".into()])
        .use_system_proxy(false)
        .timeout_secs(3)
        .build()
        .unwrap();

    for _ in 0..3 {
        let response = send(
            &client,
            Method::GET,
            format!("http://{addr}/pool?asset=BTC"),
            None,
        )
        .await
        .unwrap();
        assert_eq!(response.status.as_u16(), 206);
        assert_eq!(
            response.headers,
            HashMap::from([("x-receipt".into(), "receipt-53".into())])
        );
        assert_eq!(response.body, Bytes::from_static(b"abcdefg"));
    }
    peer.await.unwrap();
}

#[rstest]
#[case::moved(301, "POST", "GET", false)]
#[case::found(302, "POST", "GET", false)]
#[case::see_other(303, "PUT", "GET", false)]
#[case::temporary(307, "POST", "POST", true)]
#[case::permanent(308, "PUT", "PUT", true)]
#[case::put_found(302, "PUT", "PUT", true)]
#[tokio::test]
async fn redirect_method_body_and_credentials_match(
    #[case] status: u16,
    #[case] initial: &str,
    #[case] redirected: &str,
    #[case] retained: bool,
) {
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let destination = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let target = destination.local_addr().unwrap();
        let peer = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            read_headers(&mut stream).await;
            let mut body = [0; 7];
            stream.read_exact(&mut body).await.unwrap();
            assert_eq!(&body, b"payload");
            stream.write_all(format!("HTTP/1.1 {status} Redirect\r\nContent-Length: 0\r\nLocation: http://{target}/next\r\n\r\n").as_bytes()).await.unwrap();
            let (mut stream, _) = destination.accept().await.unwrap();
            let headers = read_headers(&mut stream).await;
            let mut body = Vec::new();
            if retained {
                body.resize(7, 0);
                stream.read_exact(&mut body).await.unwrap();
            }
            stream
                .write_all(b"HTTP/1.1 201 Created\r\nContent-Length: 2\r\n\r\nok")
                .await
                .unwrap();
            (headers, body)
        });
        let defaults = HashMap::from([
            ("authorization".into(), "Bearer private-27".into()),
            ("cookie".into(), "session=private-91".into()),
            ("content-type".into(), "application/octet-stream".into()),
        ]);
        let client = HttpClient::builder()
            .headers(defaults.clone())
            .use_system_proxy(false)
            .timeout_secs(3)
            .build()
            .unwrap();
        let response = send(
            &client,
            initial.parse().unwrap(),
            format!("http://{addr}/start#fragment"),
            Some(b"payload".to_vec()),
        )
        .await
        .unwrap();
        let (headers, body) = peer.await.unwrap();
        assert_eq!(response.status.as_u16(), 201);
        assert_eq!(response.body, Bytes::from_static(b"ok"));
        assert_eq!(response.headers, HashMap::new());
        assert!(
            headers.starts_with(&format!("{redirected} /next HTTP/1.1\r\n")),
            "{headers}"
        );
        assert!(!headers.contains("private-"), "{headers}");
        assert!(
            headers.contains(&format!("referer: http://{addr}/start\r\n")),
            "{headers}"
        );
        assert_eq!(
            headers.contains("content-type: application/octet-stream"),
            retained
        );
        assert_eq!(
            body,
            if retained {
                b"payload".to_vec()
            } else {
                vec![]
            }
        );
    }
}

#[rstest]
#[case::ten_redirects(10, true)]
#[case::eleven_redirects(11, false)]
#[tokio::test]
async fn redirect_limit_preserves_same_origin_credentials(
    #[case] redirects: usize,
    #[case] success: bool,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        for step in 0..=redirects.min(10) {
            let headers = read_headers(&mut stream).await;
            assert!(
                headers.starts_with(&format!("GET /{step} HTTP/1.1\r\n")),
                "{headers}"
            );
            assert!(
                headers.contains("authorization: Bearer token-37\r\n"),
                "{headers}"
            );
            assert!(headers.contains("cookie: session=91\r\n"), "{headers}");
            let response = if step == redirects {
                "HTTP/1.1 200 OK\r\nContent-Length: 4\r\n\r\ndone".to_string()
            } else {
                format!(
                    "HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: /{}\r\n\r\n",
                    step + 1
                )
            };
            stream.write_all(response.as_bytes()).await.unwrap();
        }
    });
    let client = HttpClient::builder()
        .headers(HashMap::from([
            ("authorization".into(), "Bearer token-37".into()),
            ("cookie".into(), "session=91".into()),
        ]))
        .use_system_proxy(false)
        .timeout_secs(3)
        .build()
        .unwrap();

    let result = send(&client, Method::GET, format!("http://{addr}/0"), None).await;
    tokio::time::timeout(Duration::from_secs(3), peer)
        .await
        .unwrap()
        .unwrap();

    if success {
        let response = result.unwrap();
        assert_eq!(response.status.as_u16(), 200);
        assert_eq!(response.headers, HashMap::new());
        assert_eq!(response.body.as_ref(), b"done");
    } else {
        assert!(
            matches!(result, Err(HttpClientError::Error(ref message)) if message == "too many redirects"),
            "{result:?}"
        );
    }
}

#[tokio::test]
async fn redirect_rejects_unsupported_scheme() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await;
        stream.write_all(b"HTTP/1.1 302 Found\r\nContent-Length: 0\r\nLocation: ftp://127.0.0.1/file\r\n\r\n").await.unwrap();
    });
    let client = HttpClient::builder()
        .use_system_proxy(false)
        .timeout_secs(3)
        .build()
        .unwrap();

    let result = send(&client, Method::GET, format!("http://{addr}/start"), None).await;
    peer.await.unwrap();

    assert!(
        matches!(result, Err(HttpClientError::Error(ref message)) if message == "unsupported redirect URL scheme"),
        "{result:?}"
    );
}

#[rstest]
#[case::buffered(false)]
#[case::streamed(true)]
#[tokio::test]
async fn truncated_body_returns_transport_error(#[case] streamed: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\n\r\nshort")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();
    });
    let client = HttpClient::builder()
        .use_system_proxy(false)
        .timeout_secs(3)
        .build()
        .unwrap();
    let url = format!("http://{addr}/truncated");

    let error = if streamed {
        let mut response = client.get_stream(url.clone()).await.unwrap();
        loop {
            match response.chunk().await {
                Ok(Some(_)) => {}
                Ok(None) => panic!("truncated body must not end successfully"),
                Err(e) => break e,
            }
        }
    } else {
        send(&client, Method::GET, url.clone(), None)
            .await
            .unwrap_err()
    };
    peer.await.unwrap();

    let HttpClientError::TransportError(message) = error else {
        panic!("expected transport error, was {error:?}");
    };
    assert!(
        message.contains("end of file before message length reached"),
        "{message}"
    );
    assert!(message.ends_with(&format!(" for url ({url})")), "{message}");
}

#[tokio::test]
async fn body_deadline_overrides_default_and_closes_connection() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nx")
            .await
            .unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(4), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let client = HttpClient::builder()
        .timeout_secs(20)
        .use_system_proxy(false)
        .build()
        .unwrap();
    let result = client
        .get(format!("http://{addr}/slow"), None, None, Some(1), None)
        .await;
    assert!(
        matches!(result, Err(HttpClientError::TimeoutError(_))),
        "{result:?}"
    );
    peer.await.unwrap();
}

#[tokio::test]
async fn http_proxy_preserves_absolute_target_and_authentication() {
    const USERNAME: &str = "user";
    const PASSWORD: &str = "secret";

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let headers = read_headers(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 202 Accepted\r\nContent-Length: 7\r\n\r\nproxied")
            .await
            .unwrap();
        headers
    });
    let proxy = format!("http://{USERNAME}:{PASSWORD}@{addr}");
    let client = HttpClient::builder()
        .proxy_url(proxy.clone())
        .timeout_secs(3)
        .build()
        .unwrap();
    let response = send(
        &client,
        Method::GET,
        "http://127.0.0.1:9/target?asset=BTC".into(),
        None,
    )
    .await
    .unwrap();
    let headers = peer.await.unwrap();
    assert!(
        headers.starts_with("GET http://127.0.0.1:9/target?asset=BTC HTTP/1.1\r\n"),
        "{headers}"
    );
    assert!(headers.contains("host: 127.0.0.1:9\r\n"), "{headers}");
    let expected_auth = format!(
        "proxy-authorization: Basic {}\r\n",
        BASE64.encode(format!("{USERNAME}:{PASSWORD}"))
    );
    assert!(headers.contains(&expected_auth), "{headers}");
    assert_eq!(response.status.as_u16(), 202);
    assert_eq!(response.headers, HashMap::new());
    assert_eq!(response.body, Bytes::from_static(b"proxied"));
}

#[tokio::test]
async fn canceled_request_releases_partial_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (ready, received) = tokio::sync::oneshot::channel();

    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nx")
            .await
            .unwrap();
        ready.send(()).unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let client = HttpClient::builder()
        .use_system_proxy(false)
        .build()
        .unwrap();

    let request = tokio::spawn(async move {
        client
            .get(format!("http://{addr}/body"), None, None, None, None)
            .await
    });
    received.await.unwrap();
    request.abort();
    assert!(request.await.unwrap_err().is_cancelled());
    peer.await.unwrap();
}

#[tokio::test]
async fn streamed_response_exceeds_buffered_limit() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 206 Partial Content\r\nContent-Length: 8\r\n\r\nabcdefgh")
            .await
            .unwrap();
    });
    let mut client = HttpClient::builder()
        .use_system_proxy(false)
        .timeout_secs(3)
        .build()
        .unwrap();
    client.client.max_response_bytes = 4;
    let mut response = client
        .get_stream(format!("http://{addr}/large"))
        .await
        .unwrap();
    let status = response.status();
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.unwrap() {
        body.extend_from_slice(&chunk);
    }
    peer.await.unwrap();
    assert_eq!(status, http::StatusCode::PARTIAL_CONTENT);
    assert_eq!(body, b"abcdefgh");
}

#[rstest]
#[case::drop(false)]
#[case::timeout(true)]
#[tokio::test]
async fn streamed_response_releases_partial_body(#[case] timeout: bool) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let peer = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        read_headers(&mut stream).await;
        stream
            .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 5\r\n\r\nx")
            .await
            .unwrap();
        let mut byte = [0];
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(4), stream.read(&mut byte))
                .await
                .unwrap()
                .unwrap(),
            0
        );
    });
    let client = HttpClient::builder()
        .use_system_proxy(false)
        .timeout_secs(1)
        .build()
        .unwrap();
    let mut response = client
        .get_stream(format!("http://{addr}/slow"))
        .await
        .unwrap();
    assert_eq!(
        response.chunk().await.unwrap(),
        Some(Bytes::from_static(b"x"))
    );

    if timeout {
        tokio::time::sleep(Duration::from_millis(1050)).await;
        assert!(matches!(
            response.chunk().await,
            Err(HttpClientError::TimeoutError(_))
        ));
    }
    drop(response);
    peer.await.unwrap();
}

// SSL_CERT_FILE supplies isolated trust on the Unix verifier, not native Apple/Windows stores
#[cfg(all(unix, not(target_os = "android"), not(target_vendor = "apple")))]
#[rstest]
fn platform_tls_http2_and_protocol_retries() {
    use std::{
        process::Command,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };

    use http::StatusCode;

    const MARKER: &str = "NAUTILUS_HTTP_TLS_PARITY_CHILD";
    if std::env::var_os(MARKER).is_none() {
        let directory = tempfile::tempdir().unwrap();
        let key = rcgen::KeyPair::generate().unwrap();
        let cert = rcgen::CertificateParams::new(vec!["localhost".into()])
            .unwrap()
            .self_signed(&key)
            .unwrap();
        std::fs::write(directory.path().join("cert.pem"), cert.pem()).unwrap();
        std::fs::write(directory.path().join("cert.der"), cert.der()).unwrap();
        std::fs::write(directory.path().join("key.der"), key.serialize_der()).unwrap();
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "http::tests::platform_tls_http2_and_protocol_retries",
                "--nocapture",
            ])
            .env(MARKER, directory.path())
            .env("SSL_CERT_FILE", directory.path().join("cert.pem"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed"));
        return;
    }
    nautilus_cryptography::providers::install_cryptographic_provider();
    let directory = std::path::PathBuf::from(std::env::var_os(MARKER).unwrap());
    let cert =
        rustls::pki_types::CertificateDer::from(std::fs::read(directory.join("cert.der")).unwrap());
    let key = rustls::pki_types::PrivatePkcs8KeyDer::from(
        std::fs::read(directory.join("key.der")).unwrap(),
    );
    let mut config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert], key.into())
        .unwrap();
    config.alpn_protocols = vec![b"h2".to_vec()];
    let config = Arc::new(config);
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            {
                for (reason, refused, expected_requests, success) in [
                    (h2::Reason::REFUSED_STREAM, 2, 3, true),
                    (h2::Reason::REFUSED_STREAM, 3, 3, false),
                    (h2::Reason::INTERNAL_ERROR, 1, 1, false),
                ] {
                    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
                    let addr = listener.local_addr().unwrap();
                    let attempts = Arc::new(AtomicUsize::new(0));
                    let count = attempts.clone();
                    let acceptor = tokio_rustls::TlsAcceptor::from(config.clone());

                    let peer = tokio::spawn(async move {
                        let (stream, _) = listener.accept().await.unwrap();
                        let stream = acceptor.accept(stream).await.unwrap();
                        assert_eq!(stream.get_ref().1.alpn_protocol(), Some(b"h2".as_slice()));
                        let mut connection = h2::server::handshake(stream).await.unwrap();
                        while let Some(request) = connection.accept().await {
                            let (request, mut respond) = request.unwrap();
                            assert_eq!(request.method(), Method::POST);
                            assert_eq!(request.uri().path(), "/order");

                            if count.fetch_add(1, Ordering::SeqCst) < refused {
                                respond.send_reset(reason);
                            } else {
                                let response = http::Response::builder()
                                    .status(StatusCode::CREATED)
                                    .body(())
                                    .unwrap();
                                respond
                                    .send_response(response, false)
                                    .unwrap()
                                    .send_data(Bytes::from_static(b"accepted-73"), true)
                                    .unwrap();
                            }
                        }
                    });
                    let client = HttpClient::builder()
                        .use_system_proxy(false)
                        .timeout_secs(3)
                        .build()
                        .unwrap();
                    let result = send(
                        &client,
                        Method::POST,
                        format!("https://localhost:{}/order", addr.port()),
                        Some(b"order-83".to_vec()),
                    )
                    .await;
                    assert_eq!(
                        attempts.load(Ordering::SeqCst),
                        expected_requests,
                        "{result:?}"
                    );

                    if success {
                        let response = result.unwrap();
                        assert_eq!(response.status.as_u16(), 201);
                        assert_eq!(response.headers, HashMap::new());
                        assert_eq!(response.body, Bytes::from_static(b"accepted-73"));
                    } else {
                        assert!(
                            matches!(result, Err(HttpClientError::TransportError(_))),
                            "{result:?}"
                        );
                    }
                    peer.abort();
                    assert!(peer.await.unwrap_err().is_cancelled());
                }
            }
        });
}

async fn send(
    client: &HttpClient,
    method: Method,
    url: String,
    body: Option<Vec<u8>>,
) -> Result<HttpResponse, HttpClientError> {
    client
        .request(method, url, None, None, body, None, None)
        .await
}

async fn read_headers(stream: &mut tokio::net::TcpStream) -> String {
    let mut bytes = Vec::new();
    while !bytes.ends_with(b"\r\n\r\n") {
        bytes.push(stream.read_u8().await.unwrap());
    }
    String::from_utf8(bytes).unwrap()
}
