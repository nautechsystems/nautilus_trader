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

//! Reconstruction of the web application's compact `l2` book.
//!
//! The handler owns these ladders; subscription, synchronization, and recovery remain in their
//! existing layers. Removal indices refer to the preceding sorted sides, not to prices.

use std::io::Read;

use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::read::DeflateDecoder;

use super::messages::{WsBookData, WsCompactBookUpdate, WsLevelData};

const MAX_DECODED_BYTES: usize = 1_048_576;

/// The protocol baseline for one coin, independent of publication and recovery state.
#[derive(Debug)]
pub(super) struct CompactBook {
    pub data: WsBookData,
}

impl CompactBook {
    pub(super) fn new(mut data: WsBookData) -> Self {
        for (side, levels) in data.levels.iter_mut().enumerate() {
            levels.sort_unstable_by(|a, b| {
                if side == 0 {
                    b.px.cmp(&a.px)
                } else {
                    a.px.cmp(&b.px)
                }
            });

            // Compact updates do not provide order counts
            for level in levels {
                level.n = 0;
            }
        }

        Self { data }
    }

    pub(super) fn apply(&mut self, update: WsCompactBookUpdate) -> anyhow::Result<()> {
        anyhow::ensure!(update.coin == self.data.coin, "Compact book coin mismatch");

        // Validate both sides before any deletion changes their indexing
        for (levels, indices) in self.data.levels.iter().zip(&update.removed) {
            anyhow::ensure!(
                indices.iter().all(|index| *index < levels.len()),
                "Compact book removal index exceeds the preceding side"
            );
        }

        for (side, ((levels, mut indices), upserts)) in self
            .data
            .levels
            .iter_mut()
            .zip(update.removed)
            .zip(update.levels)
            .enumerate()
        {
            indices.sort_unstable();
            let mut index = 0;
            levels.retain(|_| {
                let keep = indices.binary_search(&index).is_err();
                index += 1;
                keep
            });

            for level in upserts {
                let position = levels.binary_search_by(|entry| {
                    if side == 0 {
                        level.px.cmp(&entry.px)
                    } else {
                        entry.px.cmp(&level.px)
                    }
                });
                let next = WsLevelData {
                    px: level.px,
                    sz: level.sz,
                    n: 0,
                };

                match position {
                    Ok(index) => levels[index] = next,
                    Err(index) => levels.insert(index, next),
                }
            }
        }

        // The web reducer applies updates in arrival order, including equal timestamps
        self.data.time = update.time;
        Ok(())
    }
}

pub(super) fn decode_update(encoded: &str) -> anyhow::Result<WsCompactBookUpdate> {
    anyhow::ensure!(
        encoded.len() <= MAX_DECODED_BYTES,
        "Compact book payload exceeds size limit"
    );
    let compressed = STANDARD.decode(encoded)?;
    let decoder = DeflateDecoder::new(compressed.as_slice());
    let mut decoded = Vec::new();
    decoder
        .take((MAX_DECODED_BYTES + 1) as u64)
        .read_to_end(&mut decoded)?;
    anyhow::ensure!(
        decoded.len() <= MAX_DECODED_BYTES,
        "Decoded compact book exceeds size limit"
    );
    Ok(serde_json::from_slice(&decoded)?)
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn snapshot() -> WsBookData {
        serde_json::from_str(r#"{
            "coin":"BTC","time":100,
            "levels":[
                [{"px":"100","sz":"1","n":5},{"px":"99","sz":"2","n":4},{"px":"98","sz":"3","n":3}],
                [{"px":"101","sz":"1","n":5},{"px":"102","sz":"2","n":4},{"px":"103","sz":"3","n":3}]
            ]
        }"#).unwrap()
    }

    #[rstest]
    fn removes_old_indices_before_upserting_and_replaces_quantities() {
        let mut book = CompactBook::new(snapshot());
        let update = serde_json::from_str(
            r#"{
            "c":"BTC","t":101,
            "r":[[1],[0,1]],
            "l":[[{"p":"101","s":"4"},{"p":"100","s":"7"}],[{"p":"102","s":"8"}]]
        }"#,
        )
        .unwrap();
        book.apply(update).unwrap();
        assert_eq!(
            book.data.levels[0]
                .iter()
                .map(|v| (v.px, v.sz))
                .collect::<Vec<_>>(),
            vec![
                (dec!(101), dec!(4)),
                (dec!(100), dec!(7)),
                (dec!(98), dec!(3))
            ]
        );
        assert_eq!(
            book.data.levels[1].iter().map(|v| v.px).collect::<Vec<_>>(),
            vec![dec!(102), dec!(103)]
        );
        assert!(book.data.levels.iter().flatten().all(|level| level.n == 0));
        assert_eq!(book.data.time, 101);
    }

    #[rstest]
    fn equal_timestamp_update_is_applied_like_the_web_reducer() {
        let mut book = CompactBook::new(snapshot());
        let update = serde_json::from_str(
            r#"{
            "c":"BTC","t":100,"r":[[0],[]],"l":[[],[]]
        }"#,
        )
        .unwrap();
        book.apply(update).unwrap();
        assert_eq!(book.data.levels[0].len(), 2);
        assert_eq!(book.data.levels[0][0].px, dec!(99));
    }

    #[rstest]
    fn bad_index_does_not_partially_apply_the_other_side() {
        let mut book = CompactBook::new(snapshot());
        let before = serde_json::to_value(&book.data).unwrap();
        let update = serde_json::from_str(
            r#"{
            "c":"BTC","t":101,"r":[[0],[3]],"l":[[],[]]
        }"#,
        )
        .unwrap();
        assert!(book.apply(update).is_err());
        assert_eq!(serde_json::to_value(&book.data).unwrap(), before);
    }

    #[rstest]
    #[case("!")]
    #[case("AAAA")]
    fn rejects_malformed_compressed_updates(#[case] payload: &str) {
        assert!(decode_update(payload).is_err());
    }

    #[rstest]
    fn rejects_oversized_inflated_payload() {
        use std::io::Write;

        use flate2::{Compression, write::DeflateEncoder};

        let mut encoder = DeflateEncoder::new(Vec::new(), Compression::fast());
        encoder
            .write_all(&vec![b' '; MAX_DECODED_BYTES + 1])
            .unwrap();
        let encoded = STANDARD.encode(encoder.finish().unwrap());
        let error = decode_update(&encoded).unwrap_err();
        assert!(error.to_string().contains("exceeds size limit"));
    }

    #[rstest]
    fn empty_sides_accept_price_updates() {
        let mut data = snapshot();
        data.levels = [Vec::new(), Vec::new()];
        let mut book = CompactBook::new(data);
        book.apply(
            serde_json::from_str(
                r#"{"c":"BTC","t":101,"r":[[],[]],"l":[[{"p":"99","s":"2"}],[]]}"#,
            )
            .unwrap(),
        )
        .unwrap();
        assert_eq!(book.data.levels[0][0].px, dec!(99));
        assert_eq!(book.data.levels[0][0].sz, dec!(2));
        assert!(book.data.levels[1].is_empty());
    }

    #[rstest]
    fn decodes_and_applies_a_captured_web_frame() {
        use super::super::messages::{HyperliquidWsMessage, WsCompactBookData, decode_ws_message};

        // Public mainnet frames captured from wss://api-ui.hyperliquid.xyz/ws on 2026-11-06,
        // using {"type":"l2","c":"BTC"}; the compressed update is preserved verbatim.
        let capture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/ws_l2_capture.json")).unwrap();
        let HyperliquidWsMessage::L2 {
            data: WsCompactBookData::Snapshot(snapshot),
        } = decode_ws_message(&capture["snapshot"].to_string()).unwrap()
        else {
            panic!("capture must contain a snapshot");
        };
        let HyperliquidWsMessage::L2 {
            data: WsCompactBookData::Update(update),
        } = decode_ws_message(&capture["update"].to_string()).unwrap()
        else {
            panic!("capture must contain a compressed update");
        };
        let mut book = CompactBook::new(snapshot);
        book.apply(update).unwrap();
        assert_eq!(book.data.time, 1793945698742);
        assert_eq!(book.data.levels[0].len(), 20);
        assert_eq!(book.data.levels[1].len(), 20);
        assert_eq!(
            (book.data.levels[0][0].px, book.data.levels[0][0].sz),
            (dec!(75452), dec!(13.35807))
        );
        assert_eq!(
            (book.data.levels[1][0].px, book.data.levels[1][0].sz),
            (dec!(75453), dec!(4.03799))
        );
    }
}
