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

//! Capability-aware help for the blockchain subcommands.
//!
//! The supported DEX listings are derived from the adapter's DEX registration maps and parser
//! presence, so the CLI help cannot drift from the parsers that are actually wired.

use std::fmt::Write as _;

use clap::Command;
use nautilus_blockchain::exchanges::{
    DEX_SUPPORTED_CHAINS, DexCapability, dex_capabilities_for_chain,
};

/// Attaches capability-derived `after_long_help` sections to the blockchain subcommands.
///
/// `sync-dex` lists the DEXes it can discover; `analyze-pool(s)` list the DEXes that produce
/// snapshots, marking the replay-ready and analysis-only ones.
pub(crate) fn augment_blockchain_help(command: Command) -> Command {
    command.mut_subcommand("blockchain", |blockchain| {
        blockchain
            .mut_subcommand("sync-dex", |c| c.after_long_help(render_discovery_help()))
            .mut_subcommand("analyze-pool", |c| {
                c.after_long_help(render_snapshot_help())
            })
            .mut_subcommand("analyze-pools", |c| {
                c.after_long_help(render_snapshot_help())
            })
    })
}

fn render_discovery_help() -> String {
    let mut out = String::from("Discoverable DEXes by chain (sync-dex):\n");
    out.push_str(&capability_block(|c| c.discovery, |_| String::new()));
    out.push_str(
        "\nDEXes not listed lack a PoolCreated parser, so sync-dex rejects them before syncing.",
    );
    out
}

fn render_snapshot_help() -> String {
    let mut out = String::from("Snapshot-capable DEXes by chain (analyze-pool, analyze-pools):\n");
    out.push_str(&capability_block(|c| c.snapshots, snapshot_marker));
    out.push_str(
        "\n  * replay-ready: SetFeeProtocol tracked across replay\
         \n  + analysis only: not discoverable via sync-dex, register the pool another way\
         \nDEXes not listed lack the Initialize/Swap/Mint/Burn/Collect parsers, so analyze-pool(s) \
         reject them before syncing.",
    );
    out
}

fn snapshot_marker(capability: DexCapability) -> String {
    let mut marker = String::new();
    if capability.replay_ready {
        marker.push_str(" *");
    }

    if !capability.discovery {
        marker.push_str(" +");
    }
    marker
}

fn capability_block(
    keep: impl Fn(&DexCapability) -> bool,
    marker: impl Fn(DexCapability) -> String,
) -> String {
    let mut block = String::new();
    for blockchain in DEX_SUPPORTED_CHAINS {
        let names: Vec<String> = dex_capabilities_for_chain(blockchain)
            .into_iter()
            .filter(|c| keep(c))
            .map(|c| format!("{}{}", c.dex_type, marker(c)))
            .collect();

        if names.is_empty() {
            continue;
        }
        let label = format!("{}:", blockchain.to_string().to_lowercase());
        let _ = writeln!(block, "  {label:<10}{}", names.join(", "));
    }
    block
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn discovery_help_lists_discoverable_dexes() {
        let help = render_discovery_help();

        // UniswapV2 is discovery-only (PoolCreated but no analysis parsers).
        assert!(help.contains("UniswapV2"));
        assert!(help.contains("UniswapV3"));
        // AerodromeSlipstream has no PoolCreated parser, so it is not discoverable.
        assert!(!help.contains("AerodromeSlipstream"));
        // SushiSwapV2 is registered but wires no parsers.
        assert!(!help.contains("SushiSwapV2"));
    }

    #[rstest]
    fn snapshot_help_lists_snapshot_dexes_with_markers() {
        let help = render_snapshot_help();

        assert!(help.contains("UniswapV3 *")); // Replay-ready
        assert!(help.contains("PancakeSwapV3"));
        assert!(help.contains("AerodromeSlipstream +")); // Analysis only, not discoverable
        // PancakeSwapV3 lacks SetFeeProtocol, so it is not replay-ready.
        assert!(!help.contains("PancakeSwapV3 *"));
        // Discovery-only and unsupported DEXes are absent.
        assert!(!help.contains("UniswapV2"));
        assert!(!help.contains("SushiSwapV2"));
    }
}
