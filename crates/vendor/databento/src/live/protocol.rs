//! Lower-level live interfaces exposed for those who want more customization or
//! control.
//!
//! As these are not part of the primary live API, they are less documented and
//! subject to change without warning.

use std::{
    collections::HashMap,
    fmt::{Debug, Display},
};

use dbn::{SType, Schema};
use hex::ToHex;
use sha2::{Digest, Sha256};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt};
use tracing::{debug, error, instrument};

use super::Subscription;
use crate::{ApiKey, Error};

/// Returns the host and port for the live gateway for the given dataset.
///
/// Performs no validation on `dataset`.
pub fn determine_gateway(dataset: &str) -> String {
    const DEFAULT_PORT: u16 = 13_000;

    let dataset_subdomain: String = dataset.replace('.', "-").to_ascii_lowercase();
    format!("{dataset_subdomain}.lsg.databento.com:{DEFAULT_PORT}")
}

/// The core live API protocol.
pub struct Protocol<W> {
    sender: W,
}

impl<W> Protocol<W>
where
    W: AsyncWriteExt + Unpin,
{
    /// Creates a new instance of the live API protocol that will send raw API messages
    /// to `sender`.
    pub fn new(sender: W) -> Self {
        Self { sender }
    }

    /// Conducts CRAM authentication with the live gateway. Returns the session ID.
    ///
    /// # Errors
    /// This function returns an error if the gateway fails to respond or the authentication
    /// request is rejected.
    ///
    /// # Cancel safety
    /// This method is not cancellation safe. If this method is used in a
    /// [`tokio::select!`] statement and another branch completes first, the
    /// authentication may have been only partially sent, resulting in the gateway
    /// rejecting the authentication and closing the connection.
    #[instrument(skip(self, recver, key))]
    pub async fn authenticate<R>(
        &mut self,
        recver: &mut R,
        key: &ApiKey,
        dataset: &str,
        send_ts_out: bool,
        heartbeat_interval_s: Option<i64>,
    ) -> crate::Result<String>
    where
        R: AsyncBufReadExt + Unpin,
    {
        let mut greeting = String::new();
        // Greeting
        recver.read_line(&mut greeting).await?;
        greeting.pop(); // remove newline

        debug!(greeting);
        let mut response = String::new();
        // Challenge
        recver.read_line(&mut response).await?;
        response.pop(); // remove newline

        // Parse challenge
        let challenge = Challenge::parse(&response).inspect_err(|_| {
            error!(?response, "No CRAM challenge in response from gateway");
        })?;
        debug!(%challenge, "Received CRAM challenge");

        // Send CRAM reply/auth request
        let auth_req =
            AuthRequest::new(key, dataset, send_ts_out, heartbeat_interval_s, &challenge);
        debug!(?auth_req, "Sending CRAM reply");
        self.sender.write_all(auth_req.as_bytes()).await.unwrap();

        response.clear();
        recver.read_line(&mut response).await?;
        debug!(
            auth_resp = &response[..response.len() - 1],
            "Received auth response"
        );
        response.pop(); // remove newline

        let auth_resp = AuthResponse::parse(&response)?;
        Ok(auth_resp
            .0
            .get("session_id")
            .map(|sid| (*sid).to_owned())
            .unwrap_or_default())
    }

    /// Sends one or more subscription messages for `sub` depending on the number of symbols.
    ///
    /// # Errors
    /// This function returns an error if it's unable to communicate with the gateway.
    ///
    /// # Cancel safety
    /// This method is not cancellation safe. If this method is used in a
    /// [`tokio::select!`] statement and another branch completes first, the subscription
    /// may have been partially sent, resulting in the gateway rejecting the
    /// subscription, sending an error, and closing the connection.
    pub async fn subscribe(&mut self, sub: &Subscription) -> crate::Result<()> {
        let Subscription {
            schema,
            stype_in,
            start,
            use_snapshot,
            ..
        } = &sub;

        if *use_snapshot && start.is_some() {
            return Err(Error::BadArgument {
                param_name: "use_snapshot".to_string(),
                desc: "cannot request snapshot with start time".to_string(),
            });
        }
        let start_nanos = sub.start.as_ref().map(|start| start.unix_timestamp_nanos());

        let symbol_chunks = sub.symbols.to_chunked_api_string();
        let last_chunk_idx = symbol_chunks.len() - 1;
        for (i, sym_str) in symbol_chunks.into_iter().enumerate() {
            let sub_req = SubRequest::new(
                *schema,
                *stype_in,
                start_nanos,
                *use_snapshot,
                sub.id,
                &sym_str,
                i == last_chunk_idx,
            );
            debug!(?sub_req, "Sending subscription request");
            self.sender.write_all(sub_req.as_bytes()).await?;
        }
        Ok(())
    }

    /// Sends a start session message to the live gateway.
    ///
    /// # Errors
    /// This function returns an error if it's unable to communicate with
    /// the gateway.
    ///
    /// # Cancel safety
    /// This method is not cancellation safe. If this method is used in a
    /// [`tokio::select!`] statement and another branch completes first, the live
    /// gateway may only receive a partial message, resulting in it sending an error and
    /// closing the connection.
    pub async fn start_session(&mut self) -> crate::Result<()> {
        Ok(self.sender.write_all(StartRequest.as_bytes()).await?)
    }

    /// Shuts down the inner writer.
    ///
    /// # Errors
    /// This function returns an error if the shut down did not complete successfully.
    pub async fn shutdown(&mut self) -> crate::Result<()> {
        Ok(self.sender.shutdown().await?)
    }

    /// Consumes the protocol instance and returns the inner sender.
    pub fn into_inner(self) -> W {
        self.sender
    }
}

/// A challenge request from the live gateway.
///
/// See the [raw API documentation](https://databento.com/docs/api-reference-live/gateway-control-messages/challenge-request?live=raw)
/// for more information.
#[derive(Debug)]
pub struct Challenge<'a>(&'a str);

impl<'a> Challenge<'a> {
    /// Parses a challenge request from the given raw response.
    ///
    /// # Errors
    /// Returns an error if the response does not begin with "cram=".
    // Can't use `FromStr` with lifetime
    pub fn parse(response: &'a str) -> crate::Result<Self> {
        if response.starts_with("cram=") {
            Ok(Self(response.split_once('=').unwrap().1))
        } else {
            Err(Error::internal(
                "no CRAM challenge in response from gateway",
            ))
        }
    }
}

impl Display for Challenge<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// An authentication request to be sent to the live gateway.
///
/// See the [raw API documentation](https://databento.com/docs/api-reference-live/client-control-messages/authentication-request?live=raw)
/// for more information.
pub struct AuthRequest(String);

impl AuthRequest {
    /// Creates the raw API authentication request message from the given parameters.
    pub fn new(
        key: &ApiKey,
        dataset: &str,
        send_ts_out: bool,
        heartbeat_interval_s: Option<i64>,
        challenge: &Challenge,
    ) -> Self {
        let challenge_key = format!("{challenge}|{}", key.0);
        let mut hasher = Sha256::new();
        hasher.update(challenge_key.as_bytes());
        let hashed = hasher.finalize();
        let bucket_id = key.bucket_id();
        let encoded_response = hashed.encode_hex::<String>();
        let send_ts_out = send_ts_out as u8;
        let mut req =
                format!("auth={encoded_response}-{bucket_id}|dataset={dataset}|encoding=dbn|ts_out={send_ts_out}|client=Rust {}", env!("CARGO_PKG_VERSION"));
        if let Some(heartbeat_interval_s) = heartbeat_interval_s {
            req = format!("{req}|heartbeat_interval_s={heartbeat_interval_s}");
        }
        req.push('\n');
        Self(req)
    }

    /// Returns the string slice of the request.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the request as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Debug for AuthRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Should never be empty
        write!(f, "{}", &self.0[..self.0.len() - 1])
    }
}

/// An authentication response from the live gateway.
///
/// See the [raw API documentation](https://databento.com/docs/api-reference-live/gateway-control-messages/authentication-response?live=raw)
/// for more information.
pub struct AuthResponse<'a>(HashMap<&'a str, &'a str>);

impl<'a> AuthResponse<'a> {
    /// Parses a challenge request from the given raw response.
    ///
    /// # Errors
    /// Returns an error if the response does not begin with "cram=".
    // Can't use `FromStr` with lifetime
    pub fn parse(response: &'a str) -> crate::Result<Self> {
        let auth_keys: HashMap<&'a str, &'a str> = response
            .split('|')
            .filter_map(|kvp| kvp.split_once('='))
            .collect();
        // Lack of success key also indicates something went wrong
        if auth_keys.get("success").map(|v| *v != "1").unwrap_or(true) {
            return Err(Error::Auth(
                auth_keys
                    .get("error")
                    .map(|msg| (*msg).to_owned())
                    .unwrap_or_else(|| response.to_owned()),
            ));
        }
        Ok(Self(auth_keys))
    }

    /// Returns a reference to the key-value pairs.
    pub fn get_ref(&self) -> &HashMap<&'a str, &'a str> {
        &self.0
    }
}

/// A subscription request to be sent to the live gateway.
///
/// See the [raw API documentation](https://databento.com/docs/api-reference-live/client-control-messages/subscription-request?live=raw)
/// for more information.
pub struct SubRequest(String);

impl SubRequest {
    /// Creates the raw API authentication request message from the given parameters.
    /// `symbols` is expected to already be a valid length, such as from
    /// [`Symbols::to_chunked_api_string()`](crate::Symbols::to_chunked_api_string).
    pub fn new(
        schema: Schema,
        stype_in: SType,
        start_nanos: Option<i128>,
        use_snapshot: bool,
        id: Option<u32>,
        symbols: &str,
        is_last: bool,
    ) -> Self {
        let use_snapshot = use_snapshot as u8;
        let is_last = is_last as u8;
        let mut args = format!(
            "schema={schema}|stype_in={stype_in}|symbols={symbols}|snapshot={use_snapshot}|is_last={is_last}"
        );

        if let Some(start) = start_nanos {
            args = format!("{args}|start={start}");
        }
        if let Some(id) = id {
            args = format!("{args}|id={id}");
        }
        args.push('\n');
        Self(args)
    }

    /// Returns the string slice of the request.
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns the request as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Debug for SubRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Should never be empty
        write!(f, "{}", &self.0[..self.0.len() - 1])
    }
}

/// A request to begin the session to be sent to the live gateway.
///
/// See the [raw API documentation](https://databento.com/docs/api-reference-live/client-control-messages/session-start?live=raw)
/// for more information.
pub struct StartRequest;

impl StartRequest {
    /// Returns the string slice of the request.
    pub fn as_str(&self) -> &str {
        "start_session\n"
    }

    /// Returns the request as a byte slice.
    pub fn as_bytes(&self) -> &[u8] {
        self.as_str().as_bytes()
    }
}
