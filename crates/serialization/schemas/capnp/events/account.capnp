@0xfec455d315607b3f;
# Cap'n Proto schema for Nautilus account events

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";

# AccountState - represents the state of an account including balances and margins
struct AccountState {
    accountId @0 :Identifiers.AccountId;
    accountType @1 :Enums.AccountType;
    baseCurrency @2 :Types.Currency;
    balances @3 :List(Types.AccountBalance);
    margins @4 :List(Types.MarginBalance);
    isReported @5 :Bool;  # If reported by exchange vs system-calculated
    eventId @6 :Base.UUID4;
    tsEvent @7 :Base.UnixNanos;
    tsInit @8 :Base.UnixNanos;
}
