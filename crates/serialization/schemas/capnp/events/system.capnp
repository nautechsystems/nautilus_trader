@0xd986f7471fbdd1a3;
# Cap'n Proto schema for Nautilus system events

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";

# System event variants
struct SystemEvent {
    union {
        componentStateChanged @0 :ComponentStateChanged;
        tradingStateChanged @1 :TradingStateChanged;
        shutdownSystem @2 :ShutdownSystem;
    }
}

struct ComponentStateChanged {
    traderId @0 :Identifiers.TraderId;
    componentId @1 :Text;
    componentType @2 :Text;
    state @3 :Enums.ComponentState;
    config @4 :Base.StringMap;
    eventId @5 :Base.UUID4;
    tsEvent @6 :Base.UnixNanos;
    tsInit @7 :Base.UnixNanos;
}

struct TradingStateChanged {
    traderId @0 :Identifiers.TraderId;
    state @1 :Enums.TradingState;
    config @2 :Base.StringMap;
    eventId @3 :Base.UUID4;
    tsEvent @4 :Base.UnixNanos;
    tsInit @5 :Base.UnixNanos;
}

struct ShutdownSystem {
    reason @0 :Text;
}
