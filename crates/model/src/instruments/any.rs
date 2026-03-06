use enum_dispatch::enum_dispatch;
use serde::{Deserialize, Serialize};

use super::{
    Instrument, betting::BettingInstrument, binary_option::BinaryOption, cfd::Cfd,
    commodity::Commodity, crypto_future::CryptoFuture, crypto_option::CryptoOption,
    crypto_perpetual::CryptoPerpetual, currency_pair::CurrencyPair, equity::Equity,
    futures_contract::FuturesContract, futures_spread::FuturesSpread,
    index_instrument::IndexInstrument, option_contract::OptionContract,
    option_spread::OptionSpread, perpetual_contract::PerpetualContract,
};
use crate::types::{Price, Quantity};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[enum_dispatch(Instrument)]
pub enum InstrumentAny {
    Betting(BettingInstrument),
    BinaryOption(BinaryOption),
    Cfd(Cfd),
    Commodity(Commodity),
    CryptoFuture(CryptoFuture),
    CryptoOption(CryptoOption),
    CryptoPerpetual(CryptoPerpetual),
    CurrencyPair(CurrencyPair),
    Equity(Equity),
    FuturesContract(FuturesContract),
    FuturesSpread(FuturesSpread),
    IndexInstrument(IndexInstrument),
    OptionContract(OptionContract),
    OptionSpread(OptionSpread),
    PerpetualContract(PerpetualContract),
}

// TODO: Probably move this to the `Instrument` trait too
impl InstrumentAny {
    #[must_use]
    pub fn get_base_quantity(&self, quantity: Quantity, last_px: Price) -> Quantity {
        match self {
            Self::Betting(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::BinaryOption(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Cfd(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Commodity(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoFuture(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoOption(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CryptoPerpetual(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::CurrencyPair(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::Equity(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::FuturesContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::FuturesSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::IndexInstrument(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::OptionContract(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::OptionSpread(inst) => inst.calculate_base_quantity(quantity, last_px),
            Self::PerpetualContract(inst) => inst.calculate_base_quantity(quantity, last_px),
        }
    }

    /// Returns true if the instrument is a spread instrument.
    #[must_use]
    pub fn is_spread(&self) -> bool {
        matches!(self, Self::FuturesSpread(_) | Self::OptionSpread(_))
    }
}

impl PartialEq for InstrumentAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl crate::data::HasTsInit for InstrumentAny {
    fn ts_init(&self) -> nautilus_core::UnixNanos {
        Instrument::ts_init(self)
    }
}
