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

//! Bybit HTTP retry classification.

use crate::http::error::BybitHttpError;

pub(crate) fn should_retry_http(error: &BybitHttpError) -> bool {
    match error {
        BybitHttpError::NetworkError(_) => true,
        BybitHttpError::UnexpectedStatus { status, .. } => {
            is_retryable_status(*status) || *status >= 600
        }
        BybitHttpError::BybitError { error_code, .. } => *error_code == 10006,
        BybitHttpError::MissingCredentials
        | BybitHttpError::JsonError(_)
        | BybitHttpError::ValidationError(_)
        | BybitHttpError::BuildError(_)
        | BybitHttpError::Canceled(_) => false,
    }
}

const fn is_retryable_status(status: u16) -> bool {
    matches!(status, 429 | 500..=599)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(408, false)]
    #[case(425, false)]
    #[case(429, true)]
    #[case(500, true)]
    #[case(599, true)]
    #[case(600, true)]
    #[case(404, false)]
    fn http_status_classification(#[case] status: u16, #[case] expected: bool) {
        let error = BybitHttpError::UnexpectedStatus {
            status,
            body: String::new(),
        };

        assert_eq!(should_retry_http(&error), expected);
    }

    #[rstest]
    #[case(10006, true)]
    #[case(10001, false)]
    fn http_error_code_classification(#[case] error_code: i32, #[case] expected: bool) {
        let error = BybitHttpError::BybitError {
            error_code,
            message: String::new(),
        };

        assert_eq!(should_retry_http(&error), expected);
    }

    #[rstest]
    #[case(BybitHttpError::NetworkError(String::new()), true)]
    #[case(BybitHttpError::MissingCredentials, false)]
    #[case(BybitHttpError::Canceled(String::new()), false)]
    fn http_error_classification(#[case] error: BybitHttpError, #[case] expected: bool) {
        assert_eq!(should_retry_http(&error), expected);
    }
}
