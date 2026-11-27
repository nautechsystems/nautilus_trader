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

use std::{borrow::Cow, collections::HashMap, sync::Arc, time::Duration};

use alloy::primitives::B256;
use anyhow::Context;
use futures_util::Stream;
use nautilus_core::consts::NAUTILUS_USER_AGENT;
use nautilus_network::{
    http::{
        HttpClient, HttpClientError, HttpRedirectPolicy, HttpResponse, Method, StatusCode, Url,
    },
    retry::{RetryConfig, RetryError, RetryManager},
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

use super::types::{HypersyncBlock, HypersyncLog};

const REQUEST_TIMEOUT_SECS: u64 = 30;
const RATE_LIMIT_RESET_HEADER: &str = "x-ratelimit-reset";
const PAYLOAD_FALLBACK_BLOCKS: u64 = 200;

#[derive(Debug)]
pub(super) struct JsonClient {
    http: HttpClient,
    height_url: String,
    query_url: String,
    retry: RetryManager<RequestError>,
}

impl JsonClient {
    pub(super) fn new(url: &Url, token: &str) -> anyhow::Result<Self> {
        let headers = HashMap::from([
            ("authorization".to_string(), format!("Bearer {token}")),
            ("content-type".to_string(), "application/json".to_string()),
            ("user-agent".to_string(), NAUTILUS_USER_AGENT.to_string()),
        ]);
        let http = HttpClient::builder()
            .headers(headers)
            .header_keys(vec![RATE_LIMIT_RESET_HEADER.to_string()])
            .timeout_secs(REQUEST_TIMEOUT_SECS)
            .redirect_policy(HttpRedirectPolicy::Reject)
            .redact_url(true)
            .build()?;

        let retry = RetryManager::new(RetryConfig {
            max_retries: 12,
            initial_delay_ms: 200,
            max_delay_ms: 5_000,
            jitter_ms: 500,
            operation_timeout_ms: None,
            ..Default::default()
        });

        Ok(Self {
            http,
            height_url: append_path(url, "height")?,
            query_url: append_path(url, "query")?,
            retry,
        })
    }

    pub(super) async fn get_height(&self) -> anyhow::Result<u64> {
        let height: ArchiveHeight = Box::pin(self.request(Method::GET, &self.height_url, None))
            .await
            .context("request HyperSync height")?;
        Ok(height.height.unwrap_or(0))
    }

    pub(super) async fn get(&self, query: &Query) -> anyhow::Result<Page> {
        let mut query = Cow::Borrowed(query);
        loop {
            let body = serde_json::to_vec(query.as_ref()).context("encode HyperSync query")?;
            match Box::pin(self.request::<Page>(Method::POST, &self.query_url, Some(body))).await {
                Ok(page) => {
                    anyhow::ensure!(
                        page.next_block >= query.from_block
                            && query.to_block.is_none_or(|end| page.next_block <= end),
                        "HyperSync cursor {} is outside the query range starting at {}",
                        page.next_block,
                        query.from_block,
                    );
                    return Ok(page);
                }
                Err(error @ RequestError::Status { status, .. })
                    if status == StatusCode::PAYLOAD_TOO_LARGE.as_u16() =>
                {
                    query.to_mut().to_block = Some(query.smaller_bound().ok_or(error)?);
                }
                Err(e) => return Err(e.into()),
            }
        }
    }

    pub(super) fn stream(
        self: &Arc<Self>,
        mut query: Query,
    ) -> impl Stream<Item = anyhow::Result<Page>> + use<> {
        let client = Arc::clone(self);
        let single_block = query
            .to_block
            .is_some_and(|end| end.saturating_sub(query.from_block) == 1);

        async_stream::try_stream! {
            while query.to_block.is_none_or(|end| query.from_block < end) {
                let page = client.get(&query).await?;
                let next_block = page.next_block;
                let empty_single_block = single_block
                    && next_block == query.from_block
                    && page.data.iter().all(|batch| batch.blocks.is_empty() && batch.logs.is_empty());
                let past_archive = (query.to_block.is_none() || empty_single_block)
                    && match page.archive_height {
                        Some(height) => next_block > height,
                        None => next_block > client.get_height().await?,
                    };
                if empty_single_block && past_archive {
                    break;
                }
                if next_block == query.from_block && !past_archive {
                    Err(anyhow::anyhow!("HyperSync cursor stalled at block {next_block}"))?;
                }
                yield page;
                if past_archive {
                    break;
                }
                query.from_block = next_block;
            }
        }
    }

    async fn request<T: DeserializeOwned>(
        &self,
        method: Method,
        url: &str,
        body: Option<Vec<u8>>,
    ) -> Result<T, RequestError> {
        self.retry
            .invocation(
                "HyperSync request",
                || async {
                    let response = self.request_once(method.clone(), url, body.clone()).await?;
                    Ok(serde_json::from_slice(&response.body)?)
                },
                RequestError::is_retryable,
                RequestError::Retry,
            )
            .retry_delay(&RequestError::retry_delay)
            .execute()
            .await
    }

    async fn request_once(
        &self,
        method: Method,
        url: &str,
        body: Option<Vec<u8>>,
    ) -> Result<HttpResponse, RequestError> {
        let response = self
            .http
            .request(method, url.to_string(), None, None, body, None, None)
            .await?;

        if response.status.is_success() {
            return Ok(response);
        }

        let status = response.status.as_u16();

        let retry_after = (status == StatusCode::TOO_MANY_REQUESTS.as_u16()).then(|| {
            let reset = response
                .headers
                .get(RATE_LIMIT_RESET_HEADER)
                .and_then(|value| value.trim().parse::<u64>().ok())
                .unwrap_or(1);
            Duration::from_secs(reset.saturating_add(1))
        });

        Err(RequestError::Status {
            status,
            retry_after,
        })
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct Query {
    pub(super) from_block: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) to_block: Option<u64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) blocks: Vec<serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(super) logs: Vec<serde_json::Value>,
    pub(super) field_selection: FieldSelection,
}

impl Query {
    fn smaller_bound(&self) -> Option<u64> {
        let width = self.to_block.map_or(PAYLOAD_FALLBACK_BLOCKS, |end| {
            end.saturating_sub(self.from_block) / 2
        });

        let end = self.from_block.saturating_add(width);
        (end > self.from_block).then_some(end)
    }
}

#[derive(Debug, Clone, Serialize)]
pub(super) struct FieldSelection {
    pub(super) block: &'static [&'static str],
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    pub(super) log: &'static [&'static str],
}

#[derive(Debug, Deserialize)]
pub(super) struct Page {
    pub(super) archive_height: Option<u64>,
    pub(super) next_block: u64,
    pub(super) data: Vec<Batch>,
}

#[derive(Debug, Deserialize)]
pub(super) struct Batch {
    #[serde(default)]
    pub(super) blocks: Vec<HypersyncBlock>,
    #[serde(default, deserialize_with = "deserialize_logs")]
    pub(super) logs: Vec<HypersyncLog>,
}

#[derive(Debug, Deserialize)]
struct WireLog {
    #[serde(flatten)]
    log: HypersyncLog,
    topic0: Option<B256>,
    topic1: Option<B256>,
    topic2: Option<B256>,
    topic3: Option<B256>,
}

impl From<WireLog> for HypersyncLog {
    fn from(value: WireLog) -> Self {
        let mut log = value.log;
        log.topics = vec![value.topic0, value.topic1, value.topic2, value.topic3];
        log
    }
}

fn deserialize_logs<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<HypersyncLog>, D::Error> {
    let logs = Vec::<WireLog>::deserialize(deserializer)?;
    Ok(logs.into_iter().map(HypersyncLog::from).collect())
}

#[derive(Deserialize)]
struct ArchiveHeight {
    height: Option<u64>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RequestError {
    #[error(transparent)]
    Http(#[from] HttpClientError),
    #[error(transparent)]
    Retry(RetryError),
    #[error("decode HyperSync response: {0}")]
    Decode(#[from] serde_json::Error),
    #[error("HyperSync returned HTTP {status}")]
    Status {
        status: u16,
        retry_after: Option<Duration>,
    },
}

impl RequestError {
    pub(super) fn is_retryable(&self) -> bool {
        match self {
            Self::Http(error) => matches!(
                error,
                HttpClientError::TimeoutError(_) | HttpClientError::TransportError(_)
            ),
            Self::Status {
                status,
                retry_after,
            } => {
                *status == StatusCode::REQUEST_TIMEOUT.as_u16()
                    || retry_after.is_some()
                    || (500..600).contains(status)
            }
            Self::Decode(_) => true,
            Self::Retry(_) => false,
        }
    }

    pub(super) fn retry_delay(&self) -> Option<Duration> {
        match self {
            Self::Status { retry_after, .. } => *retry_after,
            _ => None,
        }
    }
}

fn append_path(url: &Url, segment: &str) -> anyhow::Result<String> {
    let mut url = url.clone();
    url.path_segments_mut()
        .map_err(|()| anyhow::anyhow!("HyperSync endpoint must have a path"))?
        .pop_if_empty()
        .push(segment);
    Ok(url.into())
}

#[cfg(test)]
#[cfg(not(madsim))]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use alloy::primitives::{Address, Bytes, U256, address, b256};
    use axum::{
        Json, Router,
        http::HeaderMap,
        response::IntoResponse,
        routing::{get, post},
    };
    use futures_util::StreamExt;
    use rstest::rstest;

    use super::*;

    const PAGE_JSON: &str = include_str!("../../test_data/hypersync/query_page.json");
    const TOKEN: &str = "6dd97af2-297c-4e8b-8c63-dfffe93f8a62";

    #[rstest]
    fn test_decode_block_and_log_page() {
        let page: Page = serde_json::from_str(PAGE_JSON).unwrap();
        let batch = &page.data[0];

        assert_eq!(page.archive_height, Some(42));
        assert_eq!(page.next_block, 41);
        assert_eq!(page.data.len(), 1);
        assert_eq!(
            batch.blocks,
            [HypersyncBlock {
                number: Some(40),
                hash: Some(b256!(
                    "1111111111111111111111111111111111111111111111111111111111111111"
                )),
                parent_hash: Some(b256!(
                    "2222222222222222222222222222222222222222222222222222222222222222"
                )),
                miner: Some(address!("3333333333333333333333333333333333333333")),
                gas_limit: Some(21_000),
                gas_used: Some(291),
                timestamp: Some(256),
                base_fee_per_gas: Some(U256::from(1_u128 << 65)),
                blob_gas_used: Some(U256::from(262_144)),
                excess_blob_gas: Some(U256::from(393_216)),
            }],
        );
        assert_eq!(
            batch.logs,
            [HypersyncLog {
                block_number: Some(40),
                address: Some(Address::repeat_byte(0x44)),
                transaction_hash: Some(B256::repeat_byte(0x55)),
                log_index: Some(7),
                transaction_index: Some(2),
                data: Some(Bytes::from_static(&[1, 2, 3])),
                topics: vec![
                    Some(B256::repeat_byte(0x66)),
                    None,
                    Some(B256::repeat_byte(0x77)),
                    None,
                ],
                ..Default::default()
            }],
        );
    }

    #[rstest]
    #[case::service_unavailable(StatusCode::SERVICE_UNAVAILABLE)]
    #[case::request_timeout(StatusCode::REQUEST_TIMEOUT)]
    #[tokio::test]
    async fn test_stream_retries_and_advances_empty_pages_through_tip(#[case] status: StatusCode) {
        let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let handler = move |headers: HeaderMap, Json(body): Json<serde_json::Value>| {
            let observed = Arc::clone(&observed);

            async move {
                assert_eq!(headers["authorization"], format!("Bearer {TOKEN}"));
                assert_eq!(
                    body["field_selection"]["block"],
                    serde_json::json!(["number"])
                );
                assert_eq!(body["blocks"], serde_json::json!([{}]));
                assert_eq!(body.get("to_block"), None);
                let from = body["from_block"].as_u64().unwrap();
                let mut observed = observed.lock().await;
                observed.push(from);
                match observed.len() {
                    1 => status.into_response(),
                    2 => ([("content-type", "application/json")], PAGE_JSON).into_response(),
                    3 => (
                        [("content-type", "application/json")],
                        include_str!("../../test_data/hypersync/empty_page.json"),
                    )
                        .into_response(),
                    _ => StatusCode::BAD_REQUEST.into_response(),
                }
            }
        };

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let route = Router::new().route("/chain/query", post(handler));
        let url = Url::parse(&format!("http://{}/chain/", listener.local_addr().unwrap())).unwrap();

        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });

        let mut client = JsonClient::new(&url, TOKEN).unwrap();

        client.retry = RetryManager::new(RetryConfig {
            max_retries: 1,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            jitter_ms: 0,
            operation_timeout_ms: None,
            ..Default::default()
        });

        let client = Arc::new(client);

        let query = Query {
            from_block: 40,
            to_block: None,
            blocks: vec![serde_json::json!({})],
            logs: vec![],
            field_selection: FieldSelection {
                block: &["number"],
                log: &[],
            },
        };

        let pages: Vec<_> = client.stream(query).collect().await;
        server.abort();

        assert_eq!(*requests.lock().await, [40, 40, 41]);
        let pages: Vec<_> = pages.into_iter().collect::<Result<_, _>>().unwrap();
        assert_eq!(pages.len(), 2);
        assert_eq!(pages[0].next_block, 41);
        assert_eq!(pages[1].next_block, 43);
        assert_eq!(pages[1].data.len(), 0);
    }

    #[tokio::test]
    async fn test_rate_limit_error_uses_server_reset() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();

        let route = Router::new().route(
            "/query",
            post(|| async {
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    [(RATE_LIMIT_RESET_HEADER, "3")],
                )
            }),
        );

        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let client = JsonClient::new(&url, TOKEN).unwrap();

        let error = client
            .request_once(Method::POST, &client.query_url, None)
            .await
            .unwrap_err();
        server.abort();

        assert_eq!(error.to_string(), "HyperSync returned HTTP 429");
        assert!(error.is_retryable());
        assert_eq!(error.retry_delay(), Some(Duration::from_secs(4)));
    }

    #[tokio::test]
    async fn test_height_retries_invalid_json() {
        let requests = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&requests);

        let route = Router::new().route(
            "/height",
            get(move || async move {
                match observed.fetch_add(1, Ordering::Relaxed) {
                    0 => "incomplete JSON",
                    _ => r#"{"height":42}"#,
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();

        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let mut client = JsonClient::new(&url, TOKEN).unwrap();

        client.retry = RetryManager::new(RetryConfig {
            max_retries: 1,
            initial_delay_ms: 1,
            max_delay_ms: 1,
            jitter_ms: 0,
            operation_timeout_ms: None,
            ..Default::default()
        });

        let height = client.get_height().await.unwrap();
        server.abort();

        assert_eq!(height, 42);
        assert_eq!(requests.load(Ordering::Relaxed), 2);
    }

    #[rstest]
    #[case::open(None, &[None, Some(240), Some(140)], Some(140))]
    #[case::bounded(Some(80), &[Some(80), Some(60), Some(50)], Some(50))]
    #[case::minimum(Some(41), &[Some(41)], None)]
    #[tokio::test]
    async fn test_query_narrows_after_payload_too_large(
        #[case] end: Option<u64>,
        #[case] expected_bounds: &[Option<u64>],
        #[case] expected_cursor: Option<u64>,
    ) {
        let requests = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);
        let route = Router::new().route(
            "/query",
            post(move |Json(body): Json<serde_json::Value>| {
                let observed = Arc::clone(&observed);

                async move {
                    let end = body.get("to_block").and_then(serde_json::Value::as_u64);
                    let mut requests = observed.lock().await;
                    requests.push(body);
                    match (requests.len(), end) {
                        (3, Some(end)) => Json(serde_json::json!({
                            "archive_height": 300,
                            "next_block": end,
                            "data": [],
                        }))
                        .into_response(),
                        _ => StatusCode::PAYLOAD_TOO_LARGE.into_response(),
                    }
                }
            }),
        );

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();

        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let client = JsonClient::new(&url, TOKEN).unwrap();

        let query = Query {
            from_block: 40,
            to_block: end,
            blocks: vec![serde_json::json!({})],
            logs: vec![serde_json::json!({"topics": [[B256::repeat_byte(0x66)]]})],
            field_selection: FieldSelection {
                block: &["number"],
                log: &["topic0"],
            },
        };

        let result = client.get(&query).await;
        server.abort();

        let requests = requests.lock().await;
        assert_eq!(requests.len(), expected_bounds.len());

        for (request, bound) in requests.iter().zip(expected_bounds) {
            assert_eq!(
                request.get("to_block").and_then(serde_json::Value::as_u64),
                *bound
            );
            assert_eq!(request["from_block"], 40);
            assert_eq!(request["blocks"], serde_json::json!([{}]));
            assert_eq!(
                request["logs"],
                serde_json::json!([{"topics": [[B256::repeat_byte(0x66)]]}])
            );
            assert_eq!(
                request["field_selection"],
                serde_json::json!({"block": ["number"], "log": ["topic0"]})
            );
        }

        assert_eq!(query.to_block, end);

        match expected_cursor {
            Some(cursor) => assert_eq!(result.unwrap().next_block, cursor),
            None => assert_eq!(
                result.unwrap_err().to_string(),
                "HyperSync returned HTTP 413"
            ),
        }
    }

    #[rstest]
    #[case::backwards(
        42,
        Some(42),
        None,
        39,
        Some("HyperSync cursor 39 is outside the query range starting at 40")
    )]
    #[case::past_end(
        42,
        Some(42),
        None,
        43,
        Some("HyperSync cursor 43 is outside the query range starting at 40")
    )]
    #[case::stalled(42, Some(42), None, 40, Some("HyperSync cursor stalled at block 40"))]
    #[case::stalled_single_block(
        41,
        Some(42),
        None,
        40,
        Some("HyperSync cursor stalled at block 40")
    )]
    #[case::range_past_archive(
        42,
        Some(39),
        None,
        40,
        Some("HyperSync cursor stalled at block 40")
    )]
    #[case::single_block_past_archive(41, Some(39), None, 40, None)]
    #[case::fallback_past_archive(41, None, Some(39), 40, None)]
    #[case::fallback_still_indexed(
        41,
        None,
        Some(40),
        40,
        Some("HyperSync cursor stalled at block 40")
    )]
    #[tokio::test]
    async fn test_bounded_stream_cursor(
        #[case] to_block: u64,
        #[case] archive_height: Option<u64>,
        #[case] fallback_height: Option<u64>,
        #[case] next_block: u64,
        #[case] expected: Option<&str>,
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = Url::parse(&format!("http://{}", listener.local_addr().unwrap())).unwrap();

        let height_requests = Arc::new(AtomicUsize::new(0));
        let observed = Arc::clone(&height_requests);

        let route = Router::new()
            .route(
                "/query",
                post(move || async move {
                    let mut page = serde_json::json!({"next_block": next_block, "data": []});
                    if let Some(height) = archive_height {
                        page["archive_height"] = height.into();
                    }

                    Json(page)
                }),
            )
            .route(
                "/height",
                get(move || {
                    let observed = Arc::clone(&observed);
                    async move {
                        observed.fetch_add(1, Ordering::Relaxed);
                        Json(serde_json::json!({"height": fallback_height}))
                    }
                }),
            );

        let server = tokio::spawn(async move { axum::serve(listener, route).await.unwrap() });
        let client = Arc::new(JsonClient::new(&url, TOKEN).unwrap());

        let query = Query {
            from_block: 40,
            to_block: Some(to_block),
            blocks: vec![serde_json::json!({})],
            logs: vec![],
            field_selection: FieldSelection {
                block: &["number"],
                log: &[],
            },
        };

        let pages: Vec<_> = client.stream(query).take(2).collect().await;
        server.abort();

        assert_eq!(
            height_requests.load(Ordering::Relaxed),
            usize::from(fallback_height.is_some())
        );

        if let Some(expected) = expected {
            assert_eq!(pages.len(), 1);
            assert_eq!(pages[0].as_ref().unwrap_err().to_string(), expected);
        } else {
            assert!(pages.is_empty());
        }
    }
}
