#![no_main]

use nautilus_kalshi::websocket::handler::advance_sid_sequence;
use nautilus_live::fuzz::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let (words, _) = data.as_chunks::<16>();
    for pair in words {
        let (previous, seq) = pair.split_at(8);
        let previous = u64::from_le_bytes(previous.try_into().unwrap());
        let seq = u64::from_le_bytes(seq.try_into().unwrap());
        let decision = advance_sid_sequence(previous, seq);
        assert_eq!(decision, advance_sid_sequence(previous, seq));
        match decision {
            None => assert!(seq <= previous, "stale sequence was accepted"),
            Some(false) => assert_eq!(seq, previous + 1, "contiguous sequence reported a gap"),
            Some(true) => assert!(
                previous.checked_add(1).is_none_or(|next| seq > next),
                "gap decision was not a skip"
            ),
        }
    }
});
