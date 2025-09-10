// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python error handling for Delta Exchange integration.

use pyo3::{create_exception, exceptions::PyException, prelude::*};

use crate::{
    http::error::DeltaExchangeHttpError,
    websocket::error::DeltaExchangeWsError,
};

// Create custom Python exception types
create_exception!(delta_exchange, DeltaExchangeError, PyException);
create_exception!(delta_exchange, DeltaExchangeHttpException, DeltaExchangeError);
create_exception!(delta_exchange, DeltaExchangeWebSocketException, DeltaExchangeError);

// HTTP-specific exceptions
create_exception!(delta_exchange, DeltaExchangeAuthenticationException, DeltaExchangeHttpException);
create_exception!(delta_exchange, DeltaExchangeRateLimitException, DeltaExchangeHttpException);
create_exception!(delta_exchange, DeltaExchangeInsufficientMarginException, DeltaExchangeHttpException);
create_exception!(delta_exchange, DeltaExchangeOrderNotFoundException, DeltaExchangeHttpException);
create_exception!(delta_exchange, DeltaExchangeInvalidParameterException, DeltaExchangeHttpException);
create_exception!(delta_exchange, DeltaExchangeMarketDisruptedException, DeltaExchangeHttpException);

// WebSocket-specific exceptions
create_exception!(delta_exchange, DeltaExchangeConnectionException, DeltaExchangeWebSocketException);
create_exception!(delta_exchange, DeltaExchangeSubscriptionException, DeltaExchangeWebSocketException);
create_exception!(delta_exchange, DeltaExchangeTimeoutException, DeltaExchangeWebSocketException);
create_exception!(delta_exchange, DeltaExchangeReconnectionException, DeltaExchangeWebSocketException);

/// Convert DeltaExchangeHttpError to appropriate Python exception.
pub fn http_error_to_py_err(error: DeltaExchangeHttpError) -> PyErr {
    let message = error.message();
    
    match error {
        DeltaExchangeHttpError::MissingCredentials => {
            DeltaExchangeAuthenticationException::new_err(message)
        }
        DeltaExchangeHttpError::CredentialError(_) => {
            DeltaExchangeAuthenticationException::new_err(message)
        }
        DeltaExchangeHttpError::AuthenticationError { .. } => {
            DeltaExchangeAuthenticationException::new_err(message)
        }
        DeltaExchangeHttpError::RateLimitError { .. } => {
            DeltaExchangeRateLimitException::new_err(message)
        }
        DeltaExchangeHttpError::InsufficientMargin { .. } => {
            DeltaExchangeInsufficientMarginException::new_err(message)
        }
        DeltaExchangeHttpError::OrderNotFound { .. } => {
            DeltaExchangeOrderNotFoundException::new_err(message)
        }
        DeltaExchangeHttpError::InvalidParameter { .. } => {
            DeltaExchangeInvalidParameterException::new_err(message)
        }
        DeltaExchangeHttpError::MarketDisrupted { .. } => {
            DeltaExchangeMarketDisruptedException::new_err(message)
        }
        DeltaExchangeHttpError::Timeout { .. } => {
            DeltaExchangeTimeoutException::new_err(message)
        }
        DeltaExchangeHttpError::ConnectionError { .. } => {
            DeltaExchangeConnectionException::new_err(message)
        }
        _ => DeltaExchangeHttpException::new_err(message),
    }
}

/// Convert DeltaExchangeWsError to appropriate Python exception.
pub fn ws_error_to_py_err(error: DeltaExchangeWsError) -> PyErr {
    let message = error.message();
    
    match error {
        DeltaExchangeWsError::ConnectionError(_) => {
            DeltaExchangeConnectionException::new_err(message)
        }
        DeltaExchangeWsError::AuthenticationError(_) => {
            DeltaExchangeAuthenticationException::new_err(message)
        }
        DeltaExchangeWsError::SubscriptionError(_) => {
            DeltaExchangeSubscriptionException::new_err(message)
        }
        DeltaExchangeWsError::TimeoutError(_) => {
            DeltaExchangeTimeoutException::new_err(message)
        }
        DeltaExchangeWsError::RateLimitError(_) => {
            DeltaExchangeRateLimitException::new_err(message)
        }
        DeltaExchangeWsError::ReconnectionError(_) => {
            DeltaExchangeReconnectionException::new_err(message)
        }
        _ => DeltaExchangeWebSocketException::new_err(message),
    }
}

/// Python wrapper for HTTP error with additional methods.
#[pyclass(name = "DeltaExchangeHttpError")]
#[derive(Debug, Clone)]
pub struct PyDeltaExchangeHttpError {
    pub error: DeltaExchangeHttpError,
}

#[pymethods]
impl PyDeltaExchangeHttpError {
    /// Check if the error is retryable.
    #[pyo3(name = "is_retryable")]
    pub fn py_is_retryable(&self) -> bool {
        self.error.is_retryable()
    }

    /// Check if the error is due to authentication issues.
    #[pyo3(name = "is_auth_error")]
    pub fn py_is_auth_error(&self) -> bool {
        self.error.is_auth_error()
    }

    /// Check if the error is due to rate limiting.
    #[pyo3(name = "is_rate_limit_error")]
    pub fn py_is_rate_limit_error(&self) -> bool {
        self.error.is_rate_limit_error()
    }

    /// Get the error message.
    #[pyo3(name = "message")]
    pub fn py_message(&self) -> String {
        self.error.message()
    }

    fn __str__(&self) -> String {
        format!("{}", self.error)
    }

    fn __repr__(&self) -> String {
        format!("DeltaExchangeHttpError({})", self.error)
    }
}

/// Python wrapper for WebSocket error with additional methods.
#[pyclass(name = "DeltaExchangeWsError")]
#[derive(Debug, Clone)]
pub struct PyDeltaExchangeWsError {
    pub error: DeltaExchangeWsError,
}

#[pymethods]
impl PyDeltaExchangeWsError {
    /// Check if the error is retryable.
    #[pyo3(name = "is_retryable")]
    pub fn py_is_retryable(&self) -> bool {
        self.error.is_retryable()
    }

    /// Check if the error is due to authentication issues.
    #[pyo3(name = "is_auth_error")]
    pub fn py_is_auth_error(&self) -> bool {
        self.error.is_auth_error()
    }

    /// Check if the error is due to rate limiting.
    #[pyo3(name = "is_rate_limit_error")]
    pub fn py_is_rate_limit_error(&self) -> bool {
        self.error.is_rate_limit_error()
    }

    /// Check if the error requires reconnection.
    #[pyo3(name = "requires_reconnection")]
    pub fn py_requires_reconnection(&self) -> bool {
        self.error.requires_reconnection()
    }

    /// Get the error message.
    #[pyo3(name = "message")]
    pub fn py_message(&self) -> String {
        self.error.message()
    }

    fn __str__(&self) -> String {
        format!("{}", self.error)
    }

    fn __repr__(&self) -> String {
        format!("DeltaExchangeWsError({})", self.error)
    }
}

/// Register all exception types with the Python module.
pub fn register_exceptions(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Base exceptions
    m.add("DeltaExchangeError", py.get_type::<DeltaExchangeError>())?;
    m.add("DeltaExchangeHttpException", py.get_type::<DeltaExchangeHttpException>())?;
    m.add("DeltaExchangeWebSocketException", py.get_type::<DeltaExchangeWebSocketException>())?;

    // HTTP-specific exceptions
    m.add("DeltaExchangeAuthenticationException", py.get_type::<DeltaExchangeAuthenticationException>())?;
    m.add("DeltaExchangeRateLimitException", py.get_type::<DeltaExchangeRateLimitException>())?;
    m.add("DeltaExchangeInsufficientMarginException", py.get_type::<DeltaExchangeInsufficientMarginException>())?;
    m.add("DeltaExchangeOrderNotFoundException", py.get_type::<DeltaExchangeOrderNotFoundException>())?;
    m.add("DeltaExchangeInvalidParameterException", py.get_type::<DeltaExchangeInvalidParameterException>())?;
    m.add("DeltaExchangeMarketDisruptedException", py.get_type::<DeltaExchangeMarketDisruptedException>())?;

    // WebSocket-specific exceptions
    m.add("DeltaExchangeConnectionException", py.get_type::<DeltaExchangeConnectionException>())?;
    m.add("DeltaExchangeSubscriptionException", py.get_type::<DeltaExchangeSubscriptionException>())?;
    m.add("DeltaExchangeTimeoutException", py.get_type::<DeltaExchangeTimeoutException>())?;
    m.add("DeltaExchangeReconnectionException", py.get_type::<DeltaExchangeReconnectionException>())?;

    // Error wrapper classes
    m.add_class::<PyDeltaExchangeHttpError>()?;
    m.add_class::<PyDeltaExchangeWsError>()?;

    Ok(())
}
