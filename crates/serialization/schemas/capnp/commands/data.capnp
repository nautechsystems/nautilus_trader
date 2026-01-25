@0xe5e2d65c5e3adf20;
# Cap'n Proto schema for Nautilus data commands

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Market = import "../data/market.capnp";
using Base = import "../common/base.capnp";

# Common header for data commands
struct DataCommandHeader {
    clientId @0 :Identifiers.ClientId;
    venue @1 :Identifiers.Venue;
    commandId @2 :Base.UUID4;
    tsInit @3 :Base.UnixNanos;
}

# Common header for data responses
struct DataResponseHeader {
    clientId @0 :Identifiers.ClientId;
    venue @1 :Identifiers.Venue;
    correlationId @2 :Base.UUID4;
    responseId @3 :Base.UUID4;
    tsInit @4 :Base.UnixNanos;
}

# Data command union
struct DataCommand {
    union {
        subscribe @0 :SubscribeCommand;
        unsubscribe @1 :UnsubscribeCommand;
        request @2 :RequestCommand;
    }
}

# Subscribe command union
struct SubscribeCommand {
    union {
        customData @0 :SubscribeCustomData;
        instrument @1 :SubscribeInstrument;
        instruments @2 :SubscribeInstruments;
        bookDeltas @3 :SubscribeBookDeltas;
        bookDepth10 @4 :SubscribeBookDepth10;
        bookSnapshots @5 :SubscribeBookSnapshots;
        quotes @6 :SubscribeQuotes;
        trades @7 :SubscribeTrades;
        bars @8 :SubscribeBars;
        markPrices @9 :SubscribeMarkPrices;
        indexPrices @10 :SubscribeIndexPrices;
        fundingRates @11 :SubscribeFundingRates;
        instrumentStatus @12 :SubscribeInstrumentStatus;
        instrumentClose @13 :SubscribeInstrumentClose;
    }
}

# Unsubscribe command union
struct UnsubscribeCommand {
    union {
        customData @0 :UnsubscribeCustomData;
        instrument @1 :UnsubscribeInstrument;
        instruments @2 :UnsubscribeInstruments;
        bookDeltas @3 :UnsubscribeBookDeltas;
        bookDepth10 @4 :UnsubscribeBookDepth10;
        bookSnapshots @5 :UnsubscribeBookSnapshots;
        quotes @6 :UnsubscribeQuotes;
        trades @7 :UnsubscribeTrades;
        bars @8 :UnsubscribeBars;
        markPrices @9 :UnsubscribeMarkPrices;
        indexPrices @10 :UnsubscribeIndexPrices;
        fundingRates @11 :UnsubscribeFundingRates;
        instrumentStatus @12 :UnsubscribeInstrumentStatus;
        instrumentClose @13 :UnsubscribeInstrumentClose;
    }
}

# Request command union
struct RequestCommand {
    union {
        customData @0 :RequestCustomData;
        instrument @1 :RequestInstrument;
        instruments @2 :RequestInstruments;
        bookSnapshot @3 :RequestBookSnapshot;
        bookDepth @4 :RequestBookDepth;
        quotes @5 :RequestQuotes;
        trades @6 :RequestTrades;
        bars @7 :RequestBars;
    }
}

# Subscribe commands
struct SubscribeCustomData {
    header @0 :DataCommandHeader;
    dataType @1 :Text;
}

struct SubscribeInstrument {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeInstruments {
    header @0 :DataCommandHeader;
}

struct SubscribeBookDeltas {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    bookType @2 :Enums.BookType;
    depth @3 :UInt32;
}

struct SubscribeBookDepth10 {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeBookSnapshots {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    bookType @2 :Enums.BookType;
    depth @3 :UInt32;
}

struct SubscribeQuotes {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeTrades {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeBars {
    header @0 :DataCommandHeader;
    barType @1 :Market.BarType;
}

struct SubscribeMarkPrices {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeIndexPrices {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeFundingRates {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeInstrumentStatus {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct SubscribeInstrumentClose {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

# Unsubscribe commands
struct UnsubscribeCustomData {
    header @0 :DataCommandHeader;
    dataType @1 :Text;
}

struct UnsubscribeInstrument {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeInstruments {
    header @0 :DataCommandHeader;
}

struct UnsubscribeBookDeltas {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeBookDepth10 {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeBookSnapshots {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeQuotes {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeTrades {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeBars {
    header @0 :DataCommandHeader;
    barType @1 :Market.BarType;
}

struct UnsubscribeMarkPrices {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeIndexPrices {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeFundingRates {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeInstrumentStatus {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct UnsubscribeInstrumentClose {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

# Request commands
struct RequestCustomData {
    header @0 :DataCommandHeader;
    dataType @1 :Text;
}

struct RequestInstrument {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
}

struct RequestInstruments {
    header @0 :DataCommandHeader;
}

struct RequestBookSnapshot {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    limit @2 :UInt32;
}

struct RequestBookDepth {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    depth @2 :UInt32;
}

struct RequestQuotes {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    start @2 :Base.UnixNanos;
    end @3 :Base.UnixNanos;
    limit @4 :UInt64;
}

struct RequestTrades {
    header @0 :DataCommandHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    start @2 :Base.UnixNanos;
    end @3 :Base.UnixNanos;
    limit @4 :UInt64;
}

struct RequestBars {
    header @0 :DataCommandHeader;
    barType @1 :Market.BarType;
    start @2 :Base.UnixNanos;
    end @3 :Base.UnixNanos;
    limit @4 :UInt64;
}

# Data responses
struct DataResponse {
    union {
        customData @0 :CustomDataResponse;
        instrument @1 :InstrumentResponse;
        instruments @2 :InstrumentsResponse;
        book @3 :BookResponse;
        quotes @4 :QuotesResponse;
        trades @5 :TradesResponse;
        bars @6 :BarsResponse;
    }
}

struct CustomDataResponse {
    header @0 :DataResponseHeader;
    dataType @1 :Text;
    data @2 :Data;  # Raw bytes
}

struct InstrumentResponse {
    header @0 :DataResponseHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    instrument @2 :Data;  # Serialized instrument
}

struct InstrumentsResponse {
    header @0 :DataResponseHeader;
    instruments @1 :List(Data);  # List of serialized instruments
}

struct BookResponse {
    header @0 :DataResponseHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    book @2 :Data;  # Serialized order book
}

struct QuotesResponse {
    header @0 :DataResponseHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    quotes @2 :List(Market.QuoteTick);
}

struct TradesResponse {
    header @0 :DataResponseHeader;
    instrumentId @1 :Identifiers.InstrumentId;
    trades @2 :List(Market.TradeTick);
}

struct BarsResponse {
    header @0 :DataResponseHeader;
    barType @1 :Market.BarType;
    bars @2 :List(Market.Bar);
    partial @3 :Market.Bar;
}
