//! The Live client and related API types. Used for both real-time data and intraday historical.

mod client;
pub mod protocol;

use std::{net::SocketAddr, sync::Arc};

pub use client::Client;
use dbn::{SType, Schema, VersionUpgradePolicy};
use time::{Duration, OffsetDateTime};
use tokio::net::{lookup_host, ToSocketAddrs};
use tracing::warn;
use typed_builder::TypedBuilder;

use crate::{ApiKey, Symbols};

/// A subscription for real-time or intraday historical data.
#[derive(Debug, Clone, TypedBuilder, PartialEq, Eq)]
pub struct Subscription {
    /// The symbols of the instruments to subscribe to.
    #[builder(setter(into))]
    pub symbols: Symbols,
    /// The data record schema of data to subscribe to.
    pub schema: Schema,
    /// The symbology type of the symbols in [`symbols`](Self::symbols).
    #[builder(default = SType::RawSymbol)]
    pub stype_in: SType,
    /// If specified, requests available data since that time (inclusive), based on
    /// [`ts_event`](dbn::RecordHeader::ts_event). When `None`, only real-time data is sent.
    ///
    /// Setting this field is not supported once the session has been started with
    /// [`LiveClient::start`](crate::LiveClient::start).
    #[builder(default, setter(strip_option))]
    pub start: Option<OffsetDateTime>,
    #[doc(hidden)]
    /// Request subscription with snapshot. Defaults to `false`. Conflicts with the `start` parameter.
    #[builder(setter(strip_bool))]
    pub use_snapshot: bool,
    /// The optional numerical identifier associated with this subscription.
    #[builder(default, setter(strip_option))]
    pub id: Option<u32>,
}

#[doc(hidden)]
#[derive(Debug, Copy, Clone)]
pub struct Unset;

/// A type-safe builder for the [`LiveClient`](Client). It will not allow you to call
/// [`Self::build()`] before setting the required fields:
/// - `key`
/// - `dataset`
#[derive(Debug, Clone)]
pub struct ClientBuilder<AK, D> {
    addr: Option<Arc<Vec<SocketAddr>>>,
    key: AK,
    dataset: D,
    send_ts_out: bool,
    upgrade_policy: VersionUpgradePolicy,
    heartbeat_interval: Option<Duration>,
}

impl Default for ClientBuilder<Unset, Unset> {
    fn default() -> Self {
        Self {
            addr: None,
            key: Unset,
            dataset: Unset,
            send_ts_out: false,
            upgrade_policy: VersionUpgradePolicy::default(),
            heartbeat_interval: None,
        }
    }
}

impl<AK, D> ClientBuilder<AK, D> {
    /// Sets `ts_out`, which when enabled instructs the gateway to send a send timestamp
    /// after every record. These can be decoded with the special [`WithTsOut`](dbn::record::WithTsOut) type.
    pub fn send_ts_out(mut self, send_ts_out: bool) -> Self {
        self.send_ts_out = send_ts_out;
        self
    }

    /// Sets `upgrade_policy`, which controls how to decode data from prior DBN
    /// versions. The current default is to upgrade them to the latest version while
    /// decoding.
    pub fn upgrade_policy(mut self, upgrade_policy: VersionUpgradePolicy) -> Self {
        self.upgrade_policy = upgrade_policy;
        self
    }

    /// Sets `heartbeat_interval`, which controls the interval at which the gateway
    /// will send heartbeat records if no other data records are sent. If no heartbeat
    /// interval is configured, the gateway default will be used.
    ///
    /// Note that granularity of less than a second is not supported and will be
    /// ignored.
    pub fn heartbeat_interval(mut self, heartbeat_interval: Duration) -> Self {
        if heartbeat_interval.subsec_nanoseconds() > 0 {
            warn!(
                "heartbeat_interval subsecond precision ignored: {}ns",
                heartbeat_interval.subsec_nanoseconds()
            )
        }
        self.heartbeat_interval = Some(heartbeat_interval);
        self
    }

    /// Overrides the address of the gateway the client will connect to. This is an
    /// advanced method.
    ///
    /// # Errors
    /// This function returns an error when `addr` fails to resolve.
    pub async fn addr(mut self, addr: impl ToSocketAddrs) -> crate::Result<Self> {
        const PARAM_NAME: &str = "addr";
        let addrs: Vec<_> = lookup_host(addr)
            .await
            .map_err(|e| crate::Error::bad_arg(PARAM_NAME, format!("{e}")))?
            .collect();
        self.addr = Some(Arc::new(addrs));
        Ok(self)
    }
}

impl ClientBuilder<Unset, Unset> {
    /// Creates a new [`ClientBuilder`].
    pub fn new() -> Self {
        Self::default()
    }
}

impl<D> ClientBuilder<Unset, D> {
    /// Sets the API key.
    ///
    /// # Errors
    /// This function returns an error when the API key is invalid.
    pub fn key(self, key: impl ToString) -> crate::Result<ClientBuilder<ApiKey, D>> {
        Ok(ClientBuilder {
            addr: self.addr,
            key: ApiKey::new(key.to_string())?,
            dataset: self.dataset,
            send_ts_out: self.send_ts_out,
            upgrade_policy: self.upgrade_policy,
            heartbeat_interval: self.heartbeat_interval,
        })
    }

    /// Sets the API key reading it from the `DATABENTO_API_KEY` environment
    /// variable.
    ///
    /// # Errors
    /// This function returns an error when the environment variable is not set or the
    /// API key is invalid.
    pub fn key_from_env(self) -> crate::Result<ClientBuilder<ApiKey, D>> {
        let key = crate::key_from_env()?;
        self.key(key)
    }
}

impl<AK> ClientBuilder<AK, Unset> {
    /// Sets the dataset.
    pub fn dataset(self, dataset: impl ToString) -> ClientBuilder<AK, String> {
        ClientBuilder {
            addr: self.addr,
            key: self.key,
            dataset: dataset.to_string(),
            send_ts_out: self.send_ts_out,
            upgrade_policy: self.upgrade_policy,
            heartbeat_interval: self.heartbeat_interval,
        }
    }
}

impl ClientBuilder<ApiKey, String> {
    /// Initializes the client and attempts to connect to the gateway.
    ///
    /// # Errors
    /// This function returns an error when its unable
    /// to connect and authenticate with the Live gateway.
    pub async fn build(self) -> crate::Result<Client> {
        if let Some(addr) = self.addr {
            Client::connect_with_addr(
                addr.as_slice(),
                self.key.0,
                self.dataset,
                self.send_ts_out,
                self.upgrade_policy,
                self.heartbeat_interval,
            )
            .await
        } else {
            Client::connect(
                self.key.0,
                self.dataset,
                self.send_ts_out,
                self.upgrade_policy,
                self.heartbeat_interval,
            )
            .await
        }
    }
}
