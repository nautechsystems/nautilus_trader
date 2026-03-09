// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
// -------------------------------------------------------------------------------------------------

use url::Url;

use crate::common::enums::BitgetEnvironment;

const HTTP_BASE_URL_MAINNET: &str = "https://api.bitget.com";
const HTTP_BASE_URL_DEMO: &str = "https://api.bitget.com";
const WS_PUBLIC_URL_MAINNET: &str = "wss://ws.bitget.com/v2/ws/public";
const WS_PUBLIC_URL_DEMO: &str = "wss://wspap.bitget.com/v2/ws/public";
const WS_PRIVATE_URL_MAINNET: &str = "wss://ws.bitget.com/v2/ws/private";
const WS_PRIVATE_URL_DEMO: &str = "wss://wspap.bitget.com/v2/ws/private";

/// Returns the Bitget REST base URL for the given environment.
///
/// # Panics
///
/// Panics if one of the compile-time URL constants is malformed.
#[must_use]
pub fn get_http_base_url(environment: BitgetEnvironment) -> Url {
    match environment {
        BitgetEnvironment::Mainnet => Url::parse(HTTP_BASE_URL_MAINNET).expect("valid URL"),
        BitgetEnvironment::Demo => Url::parse(HTTP_BASE_URL_DEMO).expect("valid URL"),
    }
}

/// Returns the Bitget public WebSocket URL for the given environment.
///
/// # Panics
///
/// Panics if one of the compile-time URL constants is malformed.
#[must_use]
pub fn get_ws_public_url(environment: BitgetEnvironment) -> Url {
    match environment {
        BitgetEnvironment::Mainnet => Url::parse(WS_PUBLIC_URL_MAINNET).expect("valid URL"),
        BitgetEnvironment::Demo => Url::parse(WS_PUBLIC_URL_DEMO).expect("valid URL"),
    }
}

/// Returns the Bitget private WebSocket URL for the given environment.
///
/// # Panics
///
/// Panics if one of the compile-time URL constants is malformed.
#[must_use]
pub fn get_ws_private_url(environment: BitgetEnvironment) -> Url {
    match environment {
        BitgetEnvironment::Mainnet => Url::parse(WS_PRIVATE_URL_MAINNET).expect("valid URL"),
        BitgetEnvironment::Demo => Url::parse(WS_PRIVATE_URL_DEMO).expect("valid URL"),
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn resolves_urls() {
        assert_eq!(
            get_http_base_url(BitgetEnvironment::Mainnet).as_str(),
            "https://api.bitget.com/"
        );
        assert_eq!(
            get_ws_public_url(BitgetEnvironment::Mainnet).as_str(),
            "wss://ws.bitget.com/v2/ws/public"
        );
        assert_eq!(
            get_ws_private_url(BitgetEnvironment::Demo).as_str(),
            "wss://wspap.bitget.com/v2/ws/private"
        );
    }
}
