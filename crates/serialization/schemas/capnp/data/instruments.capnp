@0x9cbf8dcd934801a7;
# Cap'n Proto schema for Nautilus instrument types
#
# WARNING: This schema is not yet stable and may change without notice
# between releases. Do not depend on wire compatibility across versions.

using Identifiers = import "../common/identifiers.capnp";
using Types = import "../common/types.capnp";
using Enums = import "../common/enums.capnp";
using Base = import "../common/base.capnp";

# Additional instrument metadata. Optional JSON-encoded Params; absence means None.
# Tick scheme names and other optional text fields use the same absence convention.

struct BettingInstrument {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    eventTypeId @2 :UInt64;
    eventTypeName @3 :Text;
    competitionId @4 :UInt64;
    competitionName @5 :Text;
    eventId @6 :UInt64;
    eventName @7 :Text;
    eventCountryCode @8 :Text;
    eventOpenDate @9 :Base.UnixNanos;
    bettingType @10 :Text;
    marketId @11 :Text;
    marketName @12 :Text;
    marketType @13 :Text;
    marketStartTime @14 :Base.UnixNanos;
    selectionId @15 :UInt64;
    selectionName @16 :Text;
    selectionHandicap @17 :Float64;
    currency @18 :Types.Currency;
    pricePrecision @19 :UInt8;
    sizePrecision @20 :UInt8;
    priceIncrement @21 :Types.Price;
    sizeIncrement @22 :Types.Quantity;
    marginInit @23 :Types.Decimal;
    marginMaint @24 :Types.Decimal;
    maxQuantity @25 :Types.Quantity;  # Optional
    minQuantity @26 :Types.Quantity;  # Optional
    maxNotional @27 :Types.Money;  # Optional
    minNotional @28 :Types.Money;  # Optional
    maxPrice @29 :Types.Price;  # Optional
    minPrice @30 :Types.Price;  # Optional
    tickScheme @31 :Text;  # Optional
    info @32 :Data;  # Optional JSON-encoded Params
    tsEvent @33 :Base.UnixNanos;
    tsInit @34 :Base.UnixNanos;
}

struct BinaryOption {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    currency @3 :Types.Currency;
    activationNs @4 :Base.UnixNanos;
    expirationNs @5 :Base.UnixNanos;
    pricePrecision @6 :UInt8;
    sizePrecision @7 :UInt8;
    priceIncrement @8 :Types.Price;
    sizeIncrement @9 :Types.Quantity;
    marginInit @10 :Types.Decimal;
    marginMaint @11 :Types.Decimal;
    eventId @12 :Text;  # Optional
    outcome @13 :Text;  # Optional
    description @14 :Text;  # Optional
    maxQuantity @15 :Types.Quantity;  # Optional
    minQuantity @16 :Types.Quantity;  # Optional
    maxNotional @17 :Types.Money;  # Optional
    minNotional @18 :Types.Money;  # Optional
    maxPrice @19 :Types.Price;  # Optional
    minPrice @20 :Types.Price;  # Optional
    tickScheme @21 :Text;  # Optional
    info @22 :Data;  # Optional JSON-encoded Params
    tsEvent @23 :Base.UnixNanos;
    tsInit @24 :Base.UnixNanos;
}

struct Cfd {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    baseCurrency @3 :Types.Currency;  # Optional
    quoteCurrency @4 :Types.Currency;
    pricePrecision @5 :UInt8;
    sizePrecision @6 :UInt8;
    priceIncrement @7 :Types.Price;
    sizeIncrement @8 :Types.Quantity;
    marginInit @9 :Types.Decimal;
    marginMaint @10 :Types.Decimal;
    lotSize @11 :Types.Quantity;  # Optional
    maxQuantity @12 :Types.Quantity;  # Optional
    minQuantity @13 :Types.Quantity;  # Optional
    maxNotional @14 :Types.Money;  # Optional
    minNotional @15 :Types.Money;  # Optional
    maxPrice @16 :Types.Price;  # Optional
    minPrice @17 :Types.Price;  # Optional
    tickScheme @18 :Text;  # Optional
    info @19 :Data;  # Optional JSON-encoded Params
    tsEvent @20 :Base.UnixNanos;
    tsInit @21 :Base.UnixNanos;
}

struct Commodity {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    quoteCurrency @3 :Types.Currency;
    pricePrecision @4 :UInt8;
    sizePrecision @5 :UInt8;
    priceIncrement @6 :Types.Price;
    sizeIncrement @7 :Types.Quantity;
    marginInit @8 :Types.Decimal;
    marginMaint @9 :Types.Decimal;
    lotSize @10 :Types.Quantity;  # Optional
    maxQuantity @11 :Types.Quantity;  # Optional
    minQuantity @12 :Types.Quantity;  # Optional
    maxNotional @13 :Types.Money;  # Optional
    minNotional @14 :Types.Money;  # Optional
    maxPrice @15 :Types.Price;  # Optional
    minPrice @16 :Types.Price;  # Optional
    tickScheme @17 :Text;  # Optional
    info @18 :Data;  # Optional JSON-encoded Params
    tsEvent @19 :Base.UnixNanos;
    tsInit @20 :Base.UnixNanos;
}

struct CryptoFuture {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    underlying @2 :Types.Currency;
    quoteCurrency @3 :Types.Currency;
    settlementCurrency @4 :Types.Currency;
    isInverse @5 :Bool;
    activationNs @6 :Base.UnixNanos;
    expirationNs @7 :Base.UnixNanos;
    pricePrecision @8 :UInt8;
    sizePrecision @9 :UInt8;
    priceIncrement @10 :Types.Price;
    sizeIncrement @11 :Types.Quantity;
    multiplier @12 :Types.Quantity;
    lotSize @13 :Types.Quantity;
    marginInit @14 :Types.Decimal;
    marginMaint @15 :Types.Decimal;
    maxQuantity @16 :Types.Quantity;  # Optional
    minQuantity @17 :Types.Quantity;  # Optional
    maxNotional @18 :Types.Money;  # Optional
    minNotional @19 :Types.Money;  # Optional
    maxPrice @20 :Types.Price;  # Optional
    minPrice @21 :Types.Price;  # Optional
    tickScheme @22 :Text;  # Optional
    info @23 :Data;  # Optional JSON-encoded Params
    tsEvent @24 :Base.UnixNanos;
    tsInit @25 :Base.UnixNanos;
}

struct CryptoFuturesSpread {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    underlying @2 :Types.Currency;
    quoteCurrency @3 :Types.Currency;
    settlementCurrency @4 :Types.Currency;
    isInverse @5 :Bool;
    strategyType @6 :Text;
    activationNs @7 :Base.UnixNanos;
    expirationNs @8 :Base.UnixNanos;
    pricePrecision @9 :UInt8;
    sizePrecision @10 :UInt8;
    priceIncrement @11 :Types.Price;
    sizeIncrement @12 :Types.Quantity;
    multiplier @13 :Types.Quantity;
    lotSize @14 :Types.Quantity;
    marginInit @15 :Types.Decimal;
    marginMaint @16 :Types.Decimal;
    maxQuantity @17 :Types.Quantity;  # Optional
    minQuantity @18 :Types.Quantity;  # Optional
    maxNotional @19 :Types.Money;  # Optional
    minNotional @20 :Types.Money;  # Optional
    maxPrice @21 :Types.Price;  # Optional
    minPrice @22 :Types.Price;  # Optional
    tickScheme @23 :Text;  # Optional
    info @24 :Data;  # Optional JSON-encoded Params
    tsEvent @25 :Base.UnixNanos;
    tsInit @26 :Base.UnixNanos;
}

struct CryptoOption {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    underlying @2 :Types.Currency;
    quoteCurrency @3 :Types.Currency;
    settlementCurrency @4 :Types.Currency;
    isInverse @5 :Bool;
    optionKind @6 :Enums.OptionKind;
    strikePrice @7 :Types.Price;
    activationNs @8 :Base.UnixNanos;
    expirationNs @9 :Base.UnixNanos;
    pricePrecision @10 :UInt8;
    sizePrecision @11 :UInt8;
    priceIncrement @12 :Types.Price;
    sizeIncrement @13 :Types.Quantity;
    multiplier @14 :Types.Quantity;
    lotSize @15 :Types.Quantity;
    marginInit @16 :Types.Decimal;
    marginMaint @17 :Types.Decimal;
    maxQuantity @18 :Types.Quantity;  # Optional
    minQuantity @19 :Types.Quantity;  # Optional
    maxNotional @20 :Types.Money;  # Optional
    minNotional @21 :Types.Money;  # Optional
    maxPrice @22 :Types.Price;  # Optional
    minPrice @23 :Types.Price;  # Optional
    tickScheme @24 :Text;  # Optional
    info @25 :Data;  # Optional JSON-encoded Params
    tsEvent @26 :Base.UnixNanos;
    tsInit @27 :Base.UnixNanos;
}

struct CryptoOptionSpread {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    underlying @2 :Types.Currency;
    quoteCurrency @3 :Types.Currency;
    settlementCurrency @4 :Types.Currency;
    isInverse @5 :Bool;
    strategyType @6 :Text;
    activationNs @7 :Base.UnixNanos;
    expirationNs @8 :Base.UnixNanos;
    pricePrecision @9 :UInt8;
    sizePrecision @10 :UInt8;
    priceIncrement @11 :Types.Price;
    sizeIncrement @12 :Types.Quantity;
    multiplier @13 :Types.Quantity;
    lotSize @14 :Types.Quantity;
    marginInit @15 :Types.Decimal;
    marginMaint @16 :Types.Decimal;
    maxQuantity @17 :Types.Quantity;  # Optional
    minQuantity @18 :Types.Quantity;  # Optional
    maxNotional @19 :Types.Money;  # Optional
    minNotional @20 :Types.Money;  # Optional
    maxPrice @21 :Types.Price;  # Optional
    minPrice @22 :Types.Price;  # Optional
    tickScheme @23 :Text;  # Optional
    info @24 :Data;  # Optional JSON-encoded Params
    tsEvent @25 :Base.UnixNanos;
    tsInit @26 :Base.UnixNanos;
}

struct CryptoPerpetual {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    baseCurrency @2 :Types.Currency;
    quoteCurrency @3 :Types.Currency;
    settlementCurrency @4 :Types.Currency;
    isInverse @5 :Bool;
    pricePrecision @6 :UInt8;
    sizePrecision @7 :UInt8;
    priceIncrement @8 :Types.Price;
    sizeIncrement @9 :Types.Quantity;
    multiplier @10 :Types.Quantity;
    lotSize @11 :Types.Quantity;
    marginInit @12 :Types.Decimal;
    marginMaint @13 :Types.Decimal;
    maxQuantity @14 :Types.Quantity;  # Optional
    minQuantity @15 :Types.Quantity;  # Optional
    maxNotional @16 :Types.Money;  # Optional
    minNotional @17 :Types.Money;  # Optional
    maxPrice @18 :Types.Price;  # Optional
    minPrice @19 :Types.Price;  # Optional
    tickScheme @20 :Text;  # Optional
    info @21 :Data;  # Optional JSON-encoded Params
    tsEvent @22 :Base.UnixNanos;
    tsInit @23 :Base.UnixNanos;
}

struct CurrencyPair {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    baseCurrency @2 :Types.Currency;
    quoteCurrency @3 :Types.Currency;
    pricePrecision @4 :UInt8;
    sizePrecision @5 :UInt8;
    priceIncrement @6 :Types.Price;
    sizeIncrement @7 :Types.Quantity;
    multiplier @8 :Types.Quantity;
    lotSize @9 :Types.Quantity;  # Optional
    marginInit @10 :Types.Decimal;
    marginMaint @11 :Types.Decimal;
    maxQuantity @12 :Types.Quantity;  # Optional
    minQuantity @13 :Types.Quantity;  # Optional
    maxNotional @14 :Types.Money;  # Optional
    minNotional @15 :Types.Money;  # Optional
    maxPrice @16 :Types.Price;  # Optional
    minPrice @17 :Types.Price;  # Optional
    tickScheme @18 :Text;  # Optional
    info @19 :Data;  # Optional JSON-encoded Params
    tsEvent @20 :Base.UnixNanos;
    tsInit @21 :Base.UnixNanos;
}

struct Equity {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    isin @2 :Text;  # Optional
    currency @3 :Types.Currency;
    pricePrecision @4 :UInt8;
    priceIncrement @5 :Types.Price;
    marginInit @6 :Types.Decimal;
    marginMaint @7 :Types.Decimal;
    lotSize @8 :Types.Quantity;  # Optional
    maxQuantity @9 :Types.Quantity;  # Optional
    minQuantity @10 :Types.Quantity;  # Optional
    maxPrice @11 :Types.Price;  # Optional
    minPrice @12 :Types.Price;  # Optional
    tickScheme @13 :Text;  # Optional
    info @14 :Data;  # Optional JSON-encoded Params
    tsEvent @15 :Base.UnixNanos;
    tsInit @16 :Base.UnixNanos;
}

struct FuturesContract {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    exchange @3 :Text;  # Optional
    underlying @4 :Text;
    activationNs @5 :Base.UnixNanos;
    expirationNs @6 :Base.UnixNanos;
    currency @7 :Types.Currency;
    pricePrecision @8 :UInt8;
    priceIncrement @9 :Types.Price;
    sizeIncrement @10 :Types.Quantity;
    sizePrecision @11 :UInt8;
    multiplier @12 :Types.Quantity;
    lotSize @13 :Types.Quantity;
    marginInit @14 :Types.Decimal;
    marginMaint @15 :Types.Decimal;
    maxQuantity @16 :Types.Quantity;  # Optional
    minQuantity @17 :Types.Quantity;  # Optional
    maxPrice @18 :Types.Price;  # Optional
    minPrice @19 :Types.Price;  # Optional
    tickScheme @20 :Text;  # Optional
    info @21 :Data;  # Optional JSON-encoded Params
    tsEvent @22 :Base.UnixNanos;
    tsInit @23 :Base.UnixNanos;
}

struct FuturesSpread {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    exchange @3 :Text;  # Optional
    underlying @4 :Text;
    strategyType @5 :Text;
    activationNs @6 :Base.UnixNanos;
    expirationNs @7 :Base.UnixNanos;
    currency @8 :Types.Currency;
    pricePrecision @9 :UInt8;
    priceIncrement @10 :Types.Price;
    sizeIncrement @11 :Types.Quantity;
    sizePrecision @12 :UInt8;
    multiplier @13 :Types.Quantity;
    lotSize @14 :Types.Quantity;
    marginInit @15 :Types.Decimal;
    marginMaint @16 :Types.Decimal;
    maxQuantity @17 :Types.Quantity;  # Optional
    minQuantity @18 :Types.Quantity;  # Optional
    maxPrice @19 :Types.Price;  # Optional
    minPrice @20 :Types.Price;  # Optional
    tickScheme @21 :Text;  # Optional
    info @22 :Data;  # Optional JSON-encoded Params
    tsEvent @23 :Base.UnixNanos;
    tsInit @24 :Base.UnixNanos;
}

struct IndexInstrument {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    currency @2 :Types.Currency;
    pricePrecision @3 :UInt8;
    sizePrecision @4 :UInt8;
    priceIncrement @5 :Types.Price;
    sizeIncrement @6 :Types.Quantity;
    tickScheme @7 :Text;  # Optional
    info @8 :Data;  # Optional JSON-encoded Params
    tsEvent @9 :Base.UnixNanos;
    tsInit @10 :Base.UnixNanos;
}

struct OptionContract {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    exchange @3 :Text;  # Optional
    underlying @4 :Text;
    optionKind @5 :Enums.OptionKind;
    strikePrice @6 :Types.Price;
    activationNs @7 :Base.UnixNanos;
    expirationNs @8 :Base.UnixNanos;
    currency @9 :Types.Currency;
    pricePrecision @10 :UInt8;
    priceIncrement @11 :Types.Price;
    sizeIncrement @12 :Types.Quantity;
    sizePrecision @13 :UInt8;
    multiplier @14 :Types.Quantity;
    lotSize @15 :Types.Quantity;
    marginInit @16 :Types.Decimal;
    marginMaint @17 :Types.Decimal;
    maxQuantity @18 :Types.Quantity;  # Optional
    minQuantity @19 :Types.Quantity;  # Optional
    maxPrice @20 :Types.Price;  # Optional
    minPrice @21 :Types.Price;  # Optional
    tickScheme @22 :Text;  # Optional
    info @23 :Data;  # Optional JSON-encoded Params
    tsEvent @24 :Base.UnixNanos;
    tsInit @25 :Base.UnixNanos;
}

struct OptionSpread {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    exchange @3 :Text;  # Optional
    underlying @4 :Text;
    strategyType @5 :Text;
    activationNs @6 :Base.UnixNanos;
    expirationNs @7 :Base.UnixNanos;
    currency @8 :Types.Currency;
    pricePrecision @9 :UInt8;
    priceIncrement @10 :Types.Price;
    sizeIncrement @11 :Types.Quantity;
    sizePrecision @12 :UInt8;
    multiplier @13 :Types.Quantity;
    lotSize @14 :Types.Quantity;
    marginInit @15 :Types.Decimal;
    marginMaint @16 :Types.Decimal;
    maxQuantity @17 :Types.Quantity;  # Optional
    minQuantity @18 :Types.Quantity;  # Optional
    maxPrice @19 :Types.Price;  # Optional
    minPrice @20 :Types.Price;  # Optional
    tickScheme @21 :Text;  # Optional
    info @22 :Data;  # Optional JSON-encoded Params
    tsEvent @23 :Base.UnixNanos;
    tsInit @24 :Base.UnixNanos;
}

struct PerpetualContract {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    underlying @2 :Text;
    assetClass @3 :Enums.AssetClass;
    baseCurrency @4 :Types.Currency;  # Optional
    quoteCurrency @5 :Types.Currency;
    settlementCurrency @6 :Types.Currency;
    isInverse @7 :Bool;
    pricePrecision @8 :UInt8;
    sizePrecision @9 :UInt8;
    priceIncrement @10 :Types.Price;
    sizeIncrement @11 :Types.Quantity;
    multiplier @12 :Types.Quantity;
    lotSize @13 :Types.Quantity;
    marginInit @14 :Types.Decimal;
    marginMaint @15 :Types.Decimal;
    maxQuantity @16 :Types.Quantity;  # Optional
    minQuantity @17 :Types.Quantity;  # Optional
    maxNotional @18 :Types.Money;  # Optional
    minNotional @19 :Types.Money;  # Optional
    maxPrice @20 :Types.Price;  # Optional
    minPrice @21 :Types.Price;  # Optional
    tickScheme @22 :Text;  # Optional
    info @23 :Data;  # Optional JSON-encoded Params
    tsEvent @24 :Base.UnixNanos;
    tsInit @25 :Base.UnixNanos;
}

struct TokenizedAsset {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    assetClass @2 :Enums.AssetClass;
    baseCurrency @3 :Types.Currency;
    quoteCurrency @4 :Types.Currency;
    isin @5 :Text;  # Optional
    pricePrecision @6 :UInt8;
    sizePrecision @7 :UInt8;
    priceIncrement @8 :Types.Price;
    sizeIncrement @9 :Types.Quantity;
    multiplier @10 :Types.Quantity;
    lotSize @11 :Types.Quantity;  # Optional
    marginInit @12 :Types.Decimal;
    marginMaint @13 :Types.Decimal;
    maxQuantity @14 :Types.Quantity;  # Optional
    minQuantity @15 :Types.Quantity;  # Optional
    maxNotional @16 :Types.Money;  # Optional
    minNotional @17 :Types.Money;  # Optional
    maxPrice @18 :Types.Price;  # Optional
    minPrice @19 :Types.Price;  # Optional
    tickScheme @20 :Text;  # Optional
    info @21 :Data;  # Optional JSON-encoded Params
    tsEvent @22 :Base.UnixNanos;
    tsInit @23 :Base.UnixNanos;
}

struct SyntheticInstrument {
    id @0 :Identifiers.InstrumentId;
    pricePrecision @1 :UInt8;
    priceIncrement @2 :Types.Price;
    components @3 :List(Identifiers.InstrumentId);
    formula @4 :Text;
    tsEvent @5 :Base.UnixNanos;
    tsInit @6 :Base.UnixNanos;
}

# Discriminated instrument union matching InstrumentAny
struct InstrumentAny {
    union {
        betting @0 :BettingInstrument;
        binaryOption @1 :BinaryOption;
        cfd @2 :Cfd;
        commodity @3 :Commodity;
        cryptoFuture @4 :CryptoFuture;
        cryptoFuturesSpread @5 :CryptoFuturesSpread;
        cryptoOption @6 :CryptoOption;
        cryptoOptionSpread @7 :CryptoOptionSpread;
        cryptoPerpetual @8 :CryptoPerpetual;
        currencyPair @9 :CurrencyPair;
        equity @10 :Equity;
        futuresContract @11 :FuturesContract;
        futuresSpread @12 :FuturesSpread;
        indexInstrument @13 :IndexInstrument;
        optionContract @14 :OptionContract;
        optionSpread @15 :OptionSpread;
        perpetualContract @16 :PerpetualContract;
        tokenizedAsset @17 :TokenizedAsset;
    }
}
