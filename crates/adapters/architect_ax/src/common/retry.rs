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

//! AX HTTP retry classification.

use crate::http::error::AxHttpError;

pub(crate) fn should_retry_http(error: &AxHttpError) -> bool {
    match error {
        AxHttpError::NetworkError(_) => true,
        AxHttpError::UnexpectedStatus { status, .. } => {
            is_retryable_status(*status) || *status >= 600
        }
        AxHttpError::MissingCredentials
        | AxHttpError::MissingSessionToken
        | AxHttpError::ApiError { .. }
        | AxHttpError::JsonError(_)
        | AxHttpError::ValidationError(_)
        | AxHttpError::BuildError(_)
        | AxHttpError::Canceled(_) => false,
    }
}

const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500..=599)
}
