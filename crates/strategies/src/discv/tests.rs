use nautilus_core::UnixNanos;
use nautilus_model::{
    data::{Participant, ParticipantKind},
    identifiers::{ParticipantId, Venue},
};

use super::{config::AddrDiscoveryConfig, strategy::AddrDiscovery};

#[test]
fn collects_unique_addresses() {
    let config = AddrDiscoveryConfig::builder().max_addresses(1000).build();
    let mut strategy = AddrDiscovery::from_config(config);

    let participants = [
        Participant::new(
            ParticipantId::new("0xbuyer1"),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ),
        Participant::new(
            ParticipantId::new("0xseller1"),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ),
        Participant::new(
            ParticipantId::new("0xbuyer2"),
            Venue::new("HYPERLIQUID"),
            ParticipantKind::Wallet,
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
        ),
    ];

    use nautilus_common::actor::DataActor;
    strategy.on_participants(&participants).unwrap();

    assert_eq!(strategy.unique_count(), 3);
}
