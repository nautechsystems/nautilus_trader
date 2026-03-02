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

//! Error code trait and macro for unique, searchable error codes.

/// Trait for errors that carry a unique nautilus error code.
pub trait ErrorCode: std::error::Error {
    /// Returns the unique error code for this error instance (e.g. `"NT-0201"`).
    fn code(&self) -> &'static str;

    /// Returns the error message prefixed with the error code.
    fn coded_message(&self) -> String {
        format!("[{}] {}", self.code(), self)
    }
}

/// Implements [`ErrorCode`] for an enum error type and generates a companion
/// documentation module containing the error codes as constants.
///
/// Each variant must be mapped to a unique code string. Adding a new variant
/// without a code is a compile error (non-exhaustive match).
///
/// Optional `///` doc comments on each variant are forwarded to the generated
/// constants, making them visible in `cargo doc`.
///
/// # Example
///
/// ```ignore
/// impl_error_codes! {
///     MyError {
///         /// The requested item was not found.
///         NotFound(_) => "NT-0101",
///         /// The input was invalid.
///         Invalid => "NT-0102",
///     }
/// }
/// // Generates: pub mod my_error_codes { pub const NOT_FOUND: &str = "NT-0101"; ... }
/// ```
#[macro_export]
macro_rules! impl_error_codes {
    ($type:ident {
        $(
            $(#[doc = $doc:expr])*
            $variant:ident $( ( $($fields:tt)* ) )? => $code:expr
        ),* $(,)?
    }) => {
        impl $crate::ErrorCode for $type {
            fn code(&self) -> &'static str {
                match self {
                    $( $type::$variant $( ( $($fields)* ) )? => $code, )*
                }
            }
        }

        $crate::paste::paste! {
            #[doc = concat!("Error codes for [`", stringify!($type), "`].")]
            pub mod [<$type:snake _codes>] {
                $(
                    $(#[doc = $doc])*
                    #[doc = concat!("Error code `", $code, "` — variant [`super::", stringify!($type), "::", stringify!($variant), "`].")]
                    #[allow(non_upper_case_globals)]
                    pub const [<$variant:snake:upper>]: &str = $code;
                )*
            }
        }
    };
}
