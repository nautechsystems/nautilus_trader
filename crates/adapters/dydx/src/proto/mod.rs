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

//! Protocol buffer type definitions for dYdX v4.
//!
//! This module contains the compiled protobuf types generated from the
//! dYdX v4 protocol definitions. The types are generated at build time
//! from the `.proto` files in the `proto/` directory.

// Re-export cosmos-sdk-proto for use by generated code and consumers
#![allow(clippy::all)]
#![allow(warnings)]
pub use cosmos_sdk_proto;

// Include all generated proto modules
pub mod dydxprotocol {
    #[allow(clippy::all, warnings)]
    pub mod accountplus {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.accountplus.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod affiliates {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.affiliates.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod assets {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.assets.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod blocktime {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.blocktime.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod bridge {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.bridge.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod clob {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.clob.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod daemons {
        #[allow(clippy::all, warnings)]
        pub mod bridge {
            include!(concat!(env!("OUT_DIR"), "/dydxprotocol.daemons.bridge.rs"));
        }
        #[allow(clippy::all, warnings)]
        pub mod liquidation {
            include!(concat!(
                env!("OUT_DIR"),
                "/dydxprotocol.daemons.liquidation.rs"
            ));
        }
        #[allow(clippy::all, warnings)]
        pub mod pricefeed {
            include!(concat!(
                env!("OUT_DIR"),
                "/dydxprotocol.daemons.pricefeed.rs"
            ));
        }
    }

    #[allow(clippy::all, warnings)]
    pub mod delaymsg {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.delaymsg.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod epochs {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.epochs.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod feetiers {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.feetiers.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod govplus {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.govplus.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod indexer {
        #[allow(clippy::all, warnings)]
        pub mod events {
            include!(concat!(env!("OUT_DIR"), "/dydxprotocol.indexer.events.rs"));
        }
        #[allow(clippy::all, warnings)]
        pub mod indexer_manager {
            include!(concat!(
                env!("OUT_DIR"),
                "/dydxprotocol.indexer.indexer_manager.rs"
            ));
        }
        #[allow(clippy::all, warnings)]
        pub mod off_chain_updates {
            include!(concat!(
                env!("OUT_DIR"),
                "/dydxprotocol.indexer.off_chain_updates.rs"
            ));
        }
        #[allow(clippy::all, warnings)]
        pub mod protocol {
            #[allow(clippy::all, warnings)]
            pub mod v1 {
                include!(concat!(
                    env!("OUT_DIR"),
                    "/dydxprotocol.indexer.protocol.v1.rs"
                ));
            }
        }
        #[allow(clippy::all, warnings)]
        pub mod redis {
            include!(concat!(env!("OUT_DIR"), "/dydxprotocol.indexer.redis.rs"));
        }
        #[allow(clippy::all, warnings)]
        pub mod shared {
            include!(concat!(env!("OUT_DIR"), "/dydxprotocol.indexer.shared.rs"));
        }
        #[allow(clippy::all, warnings)]
        pub mod socks {
            include!(concat!(env!("OUT_DIR"), "/dydxprotocol.indexer.socks.rs"));
        }
    }

    #[allow(clippy::all, warnings)]
    pub mod listing {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.listing.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod perpetuals {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.perpetuals.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod prices {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.prices.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod ratelimit {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.ratelimit.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod revshare {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.revshare.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod rewards {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.rewards.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod sending {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.sending.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod stats {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.stats.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod subaccounts {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.subaccounts.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod vault {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.vault.rs"));
    }

    #[allow(clippy::all, warnings)]
    pub mod vest {
        include!(concat!(env!("OUT_DIR"), "/dydxprotocol.vest.rs"));
    }
}
