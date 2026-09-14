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

//! Streaming HTTP responses with the request's original deadline and connection ownership.

use bytes::Bytes;
use http::StatusCode;
use http_body_util::BodyExt;
use hyper::body::Incoming;
use url::Url;

use super::{HttpClientError, client::REQUEST_TIMEOUT_MESSAGE, error::transport_error};
use crate::dst::time::Instant;

/// An HTTP response whose body is consumed incrementally.
///
/// One absolute deadline covers response headers and the whole body, including time between
/// chunk reads. Dropping an unfinished response releases its body and, under simulation,
/// aborts its owned connection task. No total body limit applies.
#[derive(Debug)]
pub struct HttpResponseStream {
    pub(super) response: http::Response<Incoming>,
    pub(super) deadline: Option<Instant>,
    pub(super) url: Option<Url>,
    #[cfg(all(feature = "simulation", madsim))]
    pub(super) _connection: super::simulation::Connection,
}

impl HttpResponseStream {
    /// Returns the HTTP response status.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.response.status()
    }

    /// Reads the next body chunk, or returns `None` at the end of the body.
    ///
    /// # Errors
    ///
    /// Returns an error on a transport failure or when the original request deadline expires.
    pub async fn chunk(&mut self) -> Result<Option<Bytes>, HttpClientError> {
        read_chunk(self.response.body_mut(), self.deadline)
            .await
            .map_err(|e| response_error(e, self.url.as_ref()))
    }
}

pub(super) async fn read_chunk<B>(
    body: &mut B,
    deadline: Option<Instant>,
) -> Result<Option<Bytes>, HttpClientError>
where
    B: http_body::Body<Data = Bytes> + Unpin,
    B::Error: std::error::Error + 'static,
{
    loop {
        let frame = match deadline {
            Some(deadline) => {
                if Instant::now() >= deadline {
                    return Err(HttpClientError::TimeoutError(
                        REQUEST_TIMEOUT_MESSAGE.into(),
                    ));
                }
                tokio::select! {
                    biased;
                    () = crate::dst::time::sleep_until(deadline) => return Err(HttpClientError::TimeoutError(REQUEST_TIMEOUT_MESSAGE.into())),
                    frame = body.frame() => frame,
                }
            }
            None => body.frame().await,
        };
        let Some(frame) = frame else {
            return Ok(None);
        };

        if let Ok(chunk) = frame.map_err(|e| transport_error(&e))?.into_data() {
            return Ok(Some(chunk));
        }
    }
}

pub(super) fn response_error(error: HttpClientError, url: Option<&Url>) -> HttpClientError {
    match (error, url) {
        (HttpClientError::TransportError(message), Some(url)) => {
            HttpClientError::TransportError(format!("{message} for url ({url})"))
        }
        (error, _) => error,
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use http::{HeaderMap, HeaderValue, header::HeaderName};
    use http_body::Frame;
    use http_body_util::StreamBody;
    use rstest::rstest;

    use super::*;

    #[tokio::test]
    async fn read_chunk_skips_trailers_and_preserves_data() {
        let trailers = HeaderMap::from_iter([(
            HeaderName::from_static("x-checksum"),
            HeaderValue::from_static("receipt-83"),
        )]);
        let frames: Vec<Result<_, io::Error>> = vec![
            Ok(Frame::data(Bytes::from_static(b"first"))),
            Ok(Frame::data(Bytes::from_static(b"second"))),
            Ok(Frame::trailers(trailers)),
        ];
        let mut body = StreamBody::new(futures_util::stream::iter(frames));

        let first = read_chunk(&mut body, None).await.unwrap();
        let second = read_chunk(&mut body, None).await.unwrap();
        let end = read_chunk(&mut body, None).await.unwrap();

        assert_eq!(first, Some(Bytes::from_static(b"first")));
        assert_eq!(second, Some(Bytes::from_static(b"second")));
        assert_eq!(end, None);
    }

    #[rstest]
    #[case::transport(io::ErrorKind::UnexpectedEof, false)]
    #[case::timeout(io::ErrorKind::TimedOut, true)]
    #[tokio::test]
    async fn read_chunk_propagates_body_error(#[case] kind: io::ErrorKind, #[case] timeout: bool) {
        let frames = vec![Err::<Frame<Bytes>, _>(io::Error::new(kind, "body failure"))];
        let mut body = StreamBody::new(futures_util::stream::iter(frames));

        let error = read_chunk(&mut body, None).await.unwrap_err();

        match (error, timeout) {
            (HttpClientError::TimeoutError(message), true)
            | (HttpClientError::TransportError(message), false) => {
                assert_eq!(message, "body failure");
            }
            (error, _) => panic!("unexpected classification: {error:?}"),
        }
    }
}
