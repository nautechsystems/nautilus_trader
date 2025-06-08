//! Types for errors received from the API and occurring in the clients.
use thiserror::Error;

/// An error that can occur while working with Databento's API.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum Error {
    /// An invalid argument was passed to a function.
    #[error("bad argument `{param_name}`: {desc}")]
    BadArgument {
        /// The name of the parameter to which the bad argument was passed.
        param_name: String,
        /// The description of why the argument was invalid.
        desc: String,
    },
    /// An I/O error while reading or writing DBN or another encoding.
    #[error("I/O error: {0:?}")]
    Io(#[from] std::io::Error),
    /// An HTTP error.
    #[cfg(feature = "historical")]
    #[error("HTTP error: {0:?}")]
    Http(#[from] reqwest::Error),
    /// An error from the Databento API.
    #[cfg(feature = "historical")]
    #[error("API error: {0}")]
    Api(ApiError),
    /// An error internal to the client.
    #[error("internal error: {0}")]
    Internal(String),
    /// An error related to DBN encoding.
    #[error("DBN error: {0}")]
    Dbn(#[source] dbn::Error),
    /// An when authentication failed.
    #[error("authentication failed: {0}")]
    Auth(String),
}
/// An alias for a `Result` with [`databento::Error`](crate::Error) as the error type.
pub type Result<T> = std::result::Result<T, Error>;

/// An error from the Databento API.
#[cfg(feature = "historical")]
#[derive(Debug)]
pub struct ApiError {
    /// The request ID.
    pub request_id: Option<String>,
    /// The HTTP status code of the response.
    pub status_code: reqwest::StatusCode,
    /// The message from the Databento API.
    pub message: String,
    /// The link to documentation related to the error.
    pub docs_url: Option<String>,
}

impl Error {
    pub(crate) fn bad_arg(param_name: impl ToString, desc: impl ToString) -> Self {
        Self::BadArgument {
            param_name: param_name.to_string(),
            desc: desc.to_string(),
        }
    }

    pub(crate) fn internal(msg: impl ToString) -> Self {
        Self::Internal(msg.to_string())
    }
}

impl From<dbn::Error> for Error {
    fn from(dbn_err: dbn::Error) -> Self {
        match dbn_err {
            // Convert to our own error type.
            dbn::Error::Io { source, .. } => Self::Io(source),
            dbn_err => Self::Dbn(dbn_err),
        }
    }
}

#[cfg(feature = "historical")]
impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let doc = if let Some(ref docs_url) = self.docs_url {
            format!(" See {docs_url} for documentation.")
        } else {
            String::new()
        };
        let status = self.status_code;
        let msg = &self.message;
        if let Some(ref request_id) = self.request_id {
            write!(f, "{request_id} failed with {status} {msg}{doc}")
        } else {
            write!(f, "{status} {msg}{doc}")
        }
    }
}
