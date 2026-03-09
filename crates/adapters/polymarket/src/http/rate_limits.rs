//! REST API rate limits for Polymarket.

use std::{num::NonZeroU32, sync::LazyLock};

use nautilus_network::ratelimiter::quota::Quota;

use crate::common::consts::HTTP_RATE_LIMIT;

/// Global REST quota for Polymarket CLOB requests.
pub static POLYMARKET_REST_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_minute(NonZeroU32::new(HTTP_RATE_LIMIT).unwrap()));
