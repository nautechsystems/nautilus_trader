@0xf0a1b2c3d4e5f607;
# Cap'n Proto schema for Nautilus identifier types
#
# WARNING: This schema is not yet stable and may change without notice
# between releases. Do not depend on wire compatibility across versions.

# Base identifier types - all are interned strings (Ustr) in Rust
struct TraderId {
    value @0 :Text;
}

struct StrategyId {
    value @0 :Text;
}

struct ActorId {
    value @0 :Text;
}

struct AccountId {
    value @0 :Text;
}

struct ClientId {
    value @0 :Text;
}

struct ClientOrderId {
    value @0 :Text;
}

struct VenueOrderId {
    value @0 :Text;
}

struct TradeId {
    value @0 :Text;
}

struct PositionId {
    value @0 :Text;
}

struct ExecAlgorithmId {
    value @0 :Text;
}

struct ComponentId {
    value @0 :Text;
}

struct OrderListId {
    value @0 :Text;
}

struct Symbol {
    value @0 :Text;
}

struct Venue {
    value @0 :Text;
}

# Composite identifier
struct InstrumentId {
    symbol @0 :Symbol;
    venue @1 :Venue;
}
