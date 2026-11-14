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

//! Pooled HTTP transport with bounded protocol retries and redirect policy.

use std::{
    error::Error,
    future::Future,
    path::Path,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http::{
    HeaderValue, Request, Response,
    header::{ACCEPT, ACCEPT_ENCODING, HeaderName, PROXY_AUTHORIZATION, REFERER, USER_AGENT},
};
use http_body::Body;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::{legacy::Client as HyperClient, proxy::matcher::Matcher},
    rt::{TokioExecutor, TokioTimer},
};
use parking_lot::Mutex;
use tower_http::follow_redirect::{
    FollowRedirect,
    policy::{Action, Attempt, Policy},
};
use tower_service::Service;
use url::Url;

use super::{HttpClientError, HttpRedirectPolicy, connector::Connector, error::transport_error};
use crate::tls::create_tls_config_from_certs_dir;

#[derive(Clone, Debug)]
pub(super) struct Client {
    client: HyperClient<HttpsConnector<Connector>, Full<Bytes>>,
    proxies: Arc<Matcher>,
}

impl Client {
    pub(super) fn new(
        proxy: Option<&str>,
        use_system_proxy: bool,
        certs_dir: Option<&str>,
        user_agent: Option<HeaderValue>,
        settings: Settings,
    ) -> Result<Self, HttpClientError> {
        let mut tls = if let Some(dir) = certs_dir {
            create_tls_config_from_certs_dir(Path::new(dir), true)
                .map_err(|e| HttpClientError::ClientBuildError(format!("{e:#}")))?
        } else {
            let provider = rustls::crypto::CryptoProvider::get_default()
                .cloned()
                .ok_or_else(|| {
                    HttpClientError::ClientBuildError("TLS provider is unavailable".into())
                })?;

            let verifier = platform_verifier(&provider)?;
            rustls::ClientConfig::builder_with_provider(provider)
                .with_safe_default_protocol_versions()
                .map_err(|e| HttpClientError::ClientBuildError(e.to_string()))?
                .dangerous()
                .with_custom_certificate_verifier(verifier)
                .with_no_client_auth()
        };

        let connector = Connector::new(tls.clone(), proxy, use_system_proxy, user_agent)?;
        let proxies = Arc::clone(&connector.proxies);
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let connector = HttpsConnector::from((connector, tls));

        let client = HyperClient::builder(TokioExecutor::new())
            .pool_timer(TokioTimer::new())
            .timer(TokioTimer::new())
            .pool_max_idle_per_host(settings.pool_max_idle_per_host)
            .pool_idle_timeout(settings.pool_idle_timeout)
            .http2_keep_alive_interval(settings.keep_alive_interval)
            .http2_keep_alive_while_idle(settings.keep_alive_interval.is_some())
            .http2_initial_stream_window_size(settings.stream_window)
            .http2_initial_connection_window_size(settings.connection_window)
            // Keep after the window sizes: setting a window size turns adaptive windows off
            .http2_adaptive_window(settings.adaptive_window)
            .build(connector);
        Ok(Self { client, proxies })
    }

    pub(super) async fn send(
        &self,
        request: Request<Full<Bytes>>,
        redirects: HttpRedirectPolicy,
    ) -> Result<Response<Incoming>, HttpClientError> {
        let policy = Redirects {
            policy: redirects,
            count: 0,
            previous: None,
            cross_origin: false,
            has_body: false,
        };

        FollowRedirect::with_policy(self.clone(), policy)
            .preserve_extensions(false)
            .call(request)
            .await
    }
}

impl Service<Request<Full<Bytes>>> for Client {
    type Response = Response<Incoming>;
    type Error = HttpClientError;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, _: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, mut request: Request<Full<Bytes>>) -> Self::Future {
        if request.uri().scheme_str() == Some("http")
            && !request.headers().contains_key(PROXY_AUTHORIZATION)
            && let Some(proxy) = self.proxies.intercept(request.uri())
            && let Some(auth) = proxy.basic_auth()
        {
            request
                .headers_mut()
                .insert(PROXY_AUTHORIZATION, auth.clone());
        }

        let client = self.client.clone();
        Box::pin(async move {
            let mut retries = 0;

            loop {
                match client.request(request.clone()).await {
                    Ok(response) => return Ok(response),
                    Err(e) if retries < 2 && retryable(&e) => retries += 1,
                    Err(e) => return Err(transport_error(&e)),
                }
            }
        })
    }
}

#[derive(Clone, Copy)]
pub(super) struct Settings {
    pub(super) pool_max_idle_per_host: usize,
    pub(super) pool_idle_timeout: Duration,
    pub(super) keep_alive_interval: Option<Duration>,
    pub(super) stream_window: Option<u32>,
    pub(super) connection_window: Option<u32>,
    pub(super) adaptive_window: bool,
}

#[derive(Clone)]
struct Redirects {
    policy: HttpRedirectPolicy,
    count: usize,
    previous: Option<Url>,
    cross_origin: bool,
    has_body: bool,
}

impl Policy<Full<Bytes>, HttpClientError> for Redirects {
    fn redirect(&mut self, attempt: &Attempt<'_>) -> Result<Action, HttpClientError> {
        if self.policy == HttpRedirectPolicy::Reject {
            return Ok(Action::Stop);
        }

        if self.count == 10 {
            return Err(HttpClientError::Error("too many redirects".into()));
        }

        if !matches!(attempt.location().scheme_str(), Some("http" | "https")) {
            return Err(HttpClientError::Error(
                "unsupported redirect URL scheme".into(),
            ));
        }

        if attempt.previous().scheme_str() == Some("https")
            && attempt.location().scheme_str() == Some("http")
        {
            return Err(HttpClientError::Error(
                "redirect downgrades HTTPS to HTTP".into(),
            ));
        }

        let (Ok(previous), Ok(next)) = (
            Url::parse(&attempt.previous().to_string()),
            Url::parse(&attempt.location().to_string()),
        ) else {
            return Err(HttpClientError::Error("invalid redirect URL".into()));
        };

        let cross_origin = previous.origin() != next.origin();

        // Venue credentials travel in custom headers and signed bodies, so no body crosses origins
        if cross_origin && self.has_body {
            return Err(HttpClientError::Error(
                "cross-origin redirect of a request with a body".into(),
            ));
        }

        self.count += 1;
        self.previous = Some(previous);
        self.cross_origin = cross_origin;
        Ok(Action::Follow)
    }

    fn on_request(&mut self, request: &mut Request<Full<Bytes>>) {
        self.has_body = !request.body().is_end_stream();

        let Some(previous) = &self.previous else {
            return;
        };

        if self.cross_origin {
            let headers = request.headers_mut();
            let retained: Vec<_> = CROSS_ORIGIN_HEADERS
                .into_iter()
                .filter_map(|name| headers.remove(&name).map(|value| (name, value)))
                .collect();
            headers.clear();
            headers.extend(retained);
            return;
        }

        let mut referer = previous.clone();
        let _ = referer.set_username("");
        let _ = referer.set_password(None);
        referer.set_fragment(None);
        if let Ok(value) = referer.as_str().parse() {
            request.headers_mut().insert(REFERER, value);
        }
    }

    fn clone_body(&self, body: &Full<Bytes>) -> Option<Full<Bytes>> {
        Some(body.clone())
    }
}

const CROSS_ORIGIN_HEADERS: [HeaderName; 3] = [ACCEPT, ACCEPT_ENCODING, USER_AGENT];

// Each `Verifier::new` reloads the platform roots, which on Linux reads the CA bundle from disk,
// so concurrent first builds share one load under the lock, and a failed load is not cached.
fn platform_verifier(
    provider: &Arc<rustls::crypto::CryptoProvider>,
) -> Result<Arc<rustls_platform_verifier::Verifier>, HttpClientError> {
    static VERIFIER: Mutex<Option<Arc<rustls_platform_verifier::Verifier>>> = Mutex::new(None);

    let mut cached = VERIFIER.lock();

    if let Some(verifier) = &*cached {
        return Ok(Arc::clone(verifier));
    }

    let verifier = rustls_platform_verifier::Verifier::new(Arc::clone(provider))
        .map_err(|e| HttpClientError::ClientBuildError(e.to_string()))?;
    Ok(Arc::clone(cached.insert(Arc::new(verifier))))
}

fn retryable(e: &(dyn Error + 'static)) -> bool {
    let mut cause = e.source();
    while let Some(e) = cause {
        if let Some(e) = e.downcast_ref::<h2::Error>() {
            return e.is_remote()
                && ((e.is_go_away() && e.reason() == Some(h2::Reason::NO_ERROR))
                    || (e.is_reset() && e.reason() == Some(h2::Reason::REFUSED_STREAM)));
        }

        cause = e.source();
    }

    false
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn cross_origin_redirect_retains_only_allowed_headers() {
        let mut policy = Redirects {
            policy: HttpRedirectPolicy::Follow,
            count: 1,
            previous: Some(Url::parse("https://source.example/start?signature=SECRET").unwrap()),
            cross_origin: true,
            has_body: false,
        };

        let mut request = Request::builder()
            .uri("https://destination.example/next")
            .header(ACCEPT, "application/json")
            .header(ACCEPT_ENCODING, "gzip")
            .header(USER_AGENT, "client-41")
            .header(REFERER, "https://private.example/signed?token=PRIOR_SECRET")
            .header("authorization", "Bearer private-27")
            .header("cookie", "session=private-91")
            .header("x-venue-apikey", "private-53")
            .header("x-venue-signature", "private-67")
            .body(Full::new(Bytes::new()))
            .unwrap();

        policy.on_request(&mut request);

        let headers: Vec<_> = request
            .headers()
            .iter()
            .map(|(name, value)| (name.as_str(), value.to_str().unwrap()))
            .collect();
        assert_eq!(
            headers,
            vec![
                ("accept", "application/json"),
                ("accept-encoding", "gzip"),
                ("user-agent", "client-41"),
            ]
        );
    }
}
