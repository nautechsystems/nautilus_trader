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
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use bytes::Bytes;
use http::{
    Request, Response,
    header::{AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, REFERER, WWW_AUTHENTICATE},
};
use http_body_util::Full;
use hyper::body::Incoming;
use hyper_rustls::HttpsConnector;
use hyper_util::{
    client::{legacy::Client as HyperClient, proxy::matcher::Matcher},
    rt::{TokioExecutor, TokioTimer},
};
use tower_http::follow_redirect::{
    FollowRedirect,
    policy::{Action, Attempt, Policy},
};
use tower_service::Service;
use url::Url;

use super::{HttpClientError, HttpRedirectPolicy, connector::Connector, error::transport_error};

#[derive(Clone, Debug)]
pub(super) struct Client {
    client: HyperClient<HttpsConnector<Connector>, Full<Bytes>>,
    proxies: Arc<Matcher>,
}

impl Client {
    pub(super) fn new(
        proxy: Option<&str>,
        use_system_proxy: bool,
        settings: Settings,
    ) -> Result<Self, HttpClientError> {
        let provider = rustls::crypto::CryptoProvider::get_default()
            .cloned()
            .ok_or_else(|| {
                HttpClientError::ClientBuildError("TLS provider is unavailable".into())
            })?;
        let verifier = rustls_platform_verifier::Verifier::new(provider.clone())
            .map_err(|e| HttpClientError::ClientBuildError(e.to_string()))?;
        let mut tls = rustls::ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| HttpClientError::ClientBuildError(e.to_string()))?
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(verifier))
            .with_no_client_auth();
        let connector = Connector::new(tls.clone(), proxy, use_system_proxy)?;
        let proxies = connector.proxies.clone();
        tls.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
        let connector = HttpsConnector::from((connector, tls));

        let client = HyperClient::builder(TokioExecutor::new())
            .pool_timer(TokioTimer::new())
            .timer(TokioTimer::new())
            .pool_max_idle_per_host(settings.pool_max_idle_per_host)
            .pool_idle_timeout(settings.pool_idle_timeout)
            .http2_keep_alive_interval(settings.keep_alive_interval)
            .http2_keep_alive_while_idle(settings.keep_alive_interval.is_some())
            .http2_adaptive_window(settings.adaptive_window)
            .build(connector);
        Ok(Self { client, proxies })
    }

    pub(super) async fn send(
        &self,
        mut request: Request<Full<Bytes>>,
        redirects: HttpRedirectPolicy,
    ) -> Result<Response<Incoming>, HttpClientError> {
        if request.uri().scheme_str() == Some("http")
            && !request.headers().contains_key(PROXY_AUTHORIZATION)
            && let Some(proxy) = self.proxies.intercept(request.uri())
            && let Some(auth) = proxy.basic_auth()
        {
            request
                .headers_mut()
                .insert(PROXY_AUTHORIZATION, auth.clone());
        }

        let policy = Redirects {
            policy: redirects,
            count: 0,
            previous: None,
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

    fn call(&mut self, request: Request<Full<Bytes>>) -> Self::Future {
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
    pub(super) adaptive_window: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            pool_max_idle_per_host: usize::MAX,
            pool_idle_timeout: Duration::from_secs(90),
            keep_alive_interval: None,
            adaptive_window: false,
        }
    }
}

#[derive(Clone)]
struct Redirects {
    policy: HttpRedirectPolicy,
    count: usize,
    previous: Option<Url>,
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

        self.count += 1;
        self.previous = Url::parse(&attempt.previous().to_string()).ok();
        Ok(Action::Follow)
    }

    fn on_request(&mut self, request: &mut Request<Full<Bytes>>) {
        let Some(previous) = &self.previous else {
            return;
        };
        let Ok(next) = Url::parse(&request.uri().to_string()) else {
            return;
        };

        if previous.origin() != next.origin() {
            for name in [AUTHORIZATION, COOKIE, PROXY_AUTHORIZATION, WWW_AUTHENTICATE] {
                request.headers_mut().remove(name);
            }
            request.headers_mut().remove("cookie2");
        }

        if !(previous.scheme() == "https" && next.scheme() == "http") {
            let mut referer = previous.clone();
            let _ = referer.set_username("");
            let _ = referer.set_password(None);
            referer.set_fragment(None);
            if let Ok(value) = referer.as_str().parse() {
                request.headers_mut().insert(REFERER, value);
            }
        }
    }

    fn clone_body(&self, body: &Full<Bytes>) -> Option<Full<Bytes>> {
        Some(body.clone())
    }
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
