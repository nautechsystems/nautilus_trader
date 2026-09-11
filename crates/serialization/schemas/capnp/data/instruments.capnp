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
    makerFee @25 :Types.Decimal;
    takerFee @26 :Types.Decimal;
    maxQuantity @27 :Types.Quantity;  # Optional
    minQuantity @28 :Types.Quantity;  # Optional
    maxNotional @29 :Types.Money;  # Optional
    minNotional @30 :Types.Money;  # Optional
    maxPrice @31 :Types.Price;  # Optional
    minPrice @32 :Types.Price;  # Optional
    tickScheme @33 :Text;  # Optional
    info @34 :Data;  # Optional JSON-encoded Params
    tsEvent @35 :Base.UnixNanos;
    tsInit @36 :Base.UnixNanos;
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
    makerFee @12 :Types.Decimal;
    takerFee @13 :Types.Decimal;
    eventId @14 :Text;  # Optional
    outcome @15 :Text;  # Optional
    description @16 :Text;  # Optional
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
    makerFee @11 :Types.Decimal;
    takerFee @12 :Types.Decimal;
    lotSize @13 :Types.Quantity;  # Optional
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
    makerFee @10 :Types.Decimal;
    takerFee @11 :Types.Decimal;
    lotSize @12 :Types.Quantity;  # Optional
    maxQuantity @13 :Types.Quantity;  # Optional
    minQuantity @14 :Types.Quantity;  # Optional
    maxNotional @15 :Types.Money;  # Optional
    minNotional @16 :Types.Money;  # Optional
    maxPrice @17 :Types.Price;  # Optional
    minPrice @18 :Types.Price;  # Optional
    tickScheme @19 :Text;  # Optional
    info @20 :Data;  # Optional JSON-encoded Params
    tsEvent @21 :Base.UnixNanos;
    tsInit @22 :Base.UnixNanos;
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
    makerFee @16 :Types.Decimal;
    takerFee @17 :Types.Decimal;
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
    makerFee @17 :Types.Decimal;
    takerFee @18 :Types.Decimal;
    maxQuantity @19 :Types.Quantity;  # Optional
    minQuantity @20 :Types.Quantity;  # Optional
    maxNotional @21 :Types.Money;  # Optional
    minNotional @22 :Types.Money;  # Optional
    maxPrice @23 :Types.Price;  # Optional
    minPrice @24 :Types.Price;  # Optional
    tickScheme @25 :Text;  # Optional
    info @26 :Data;  # Optional JSON-encoded Params
    tsEvent @27 :Base.UnixNanos;
    tsInit @28 :Base.UnixNanos;
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
    makerFee @18 :Types.Decimal;
    takerFee @19 :Types.Decimal;
    maxQuantity @20 :Types.Quantity;  # Optional
    minQuantity @21 :Types.Quantity;  # Optional
    maxNotional @22 :Types.Money;  # Optional
    minNotional @23 :Types.Money;  # Optional
    maxPrice @24 :Types.Price;  # Optional
    minPrice @25 :Types.Price;  # Optional
    tickScheme @26 :Text;  # Optional
    info @27 :Data;  # Optional JSON-encoded Params
    tsEvent @28 :Base.UnixNanos;
    tsInit @29 :Base.UnixNanos;
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
    makerFee @17 :Types.Decimal;
    takerFee @18 :Types.Decimal;
    maxQuantity @19 :Types.Quantity;  # Optional
    minQuantity @20 :Types.Quantity;  # Optional
    maxNotional @21 :Types.Money;  # Optional
    minNotional @22 :Types.Money;  # Optional
    maxPrice @23 :Types.Price;  # Optional
    minPrice @24 :Types.Price;  # Optional
    tickScheme @25 :Text;  # Optional
    info @26 :Data;  # Optional JSON-encoded Params
    tsEvent @27 :Base.UnixNanos;
    tsInit @28 :Base.UnixNanos;
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
    makerFee @14 :Types.Decimal;
    takerFee @15 :Types.Decimal;
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
    makerFee @12 :Types.Decimal;
    takerFee @13 :Types.Decimal;
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

struct Equity {
    id @0 :Identifiers.InstrumentId;
    rawSymbol @1 :Identifiers.Symbol;
    isin @2 :Text;  # Optional
    currency @3 :Types.Currency;
    pricePrecision @4 :UInt8;
    priceIncrement @5 :Types.Price;
    marginInit @6 :Types.Decimal;
    marginMaint @7 :Types.Decimal;
    makerFee @8 :Types.Decimal;
    takerFee @9 :Types.Decimal;
    lotSize @10 :Types.Quantity;  # Optional
    maxQuantity @11 :Types.Quantity;  # Optional
    minQuantity @12 :Types.Quantity;  # Optional
    maxPrice @13 :Types.Price;  # Optional
    minPrice @14 :Types.Price;  # Optional
    tickScheme @15 :Text;  # Optional
    info @16 :Data;  # Optional JSON-encoded Params
    tsEvent @17 :Base.UnixNanos;
    tsInit @18 :Base.UnixNanos;
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
    makerFee @16 :Types.Decimal;
    takerFee @17 :Types.Decimal;
    maxQuantity @18 :Types.Quantity;  # Optional
    minQuantity @19 :Types.Quantity;  # Optional
    maxPrice @20 :Types.Price;  # Optional
    minPrice @21 :Types.Price;  # Optional
    tickScheme @22 :Text;  # Optional
    info @23 :Data;  # Optional JSON-encoded Params
    tsEvent @24 :Base.UnixNanos;
    tsInit @25 :Base.UnixNanos;
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
    makerFee @17 :Types.Decimal;
    takerFee @18 :Types.Decimal;
    maxQuantity @19 :Types.Quantity;  # Optional
    minQuantity @20 :Types.Quantity;  # Optional
    maxPrice @21 :Types.Price;  # Optional
    minPrice @22 :Types.Price;  # Optional
    tickScheme @23 :Text;  # Optional
    info @24 :Data;  # Optional JSON-encoded Params
    tsEvent @25 :Base.UnixNanos;
    tsInit @26 :Base.UnixNanos;
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
    makerFee @18 :Types.Decimal;
    takerFee @19 :Types.Decimal;
    maxQuantity @20 :Types.Quantity;  # Optional
    minQuantity @21 :Types.Quantity;  # Optional
    maxPrice @22 :Types.Price;  # Optional
    minPrice @23 :Types.Price;  # Optional
    tickScheme @24 :Text;  # Optional
    info @25 :Data;  # Optional JSON-encoded Params
    tsEvent @26 :Base.UnixNanos;
    tsInit @27 :Base.UnixNanos;
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
    makerFee @17 :Types.Decimal;
    takerFee @18 :Types.Decimal;
    maxQuantity @19 :Types.Quantity;  # Optional
    minQuantity @20 :Types.Quantity;  # Optional
    maxPrice @21 :Types.Price;  # Optional
    minPrice @22 :Types.Price;  # Optional
    tickScheme @23 :Text;  # Optional
    info @24 :Data;  # Optional JSON-encoded Params
    tsEvent @25 :Base.UnixNanos;
    tsInit @26 :Base.UnixNanos;
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
    makerFee @16 :Types.Decimal;
    takerFee @17 :Types.Decimal;
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
    makerFee @14 :Types.Decimal;
    takerFee @15 :Types.Decimal;
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
