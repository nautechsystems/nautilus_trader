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

//! Contract currency definitions and their DeFi presentation metadata.

use std::{str::FromStr, sync::LazyLock};

use ahash::AHashMap;
use alloy_primitives::Address;
use nautilus_core::correctness::{CorrectnessError, CorrectnessResult, check_valid_string_utf8};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use super::{Blockchain, Token};
use crate::{
    currencies::CURRENCY_MAP, enums::CurrencyType, instruments::Instrument, types::Currency,
};

static TOKEN_CURRENCIES: LazyLock<Mutex<AHashMap<Ustr, (Currency, TokenCurrency)>>> =
    LazyLock::new(|| Mutex::new(AHashMap::new()));

/// Binds a readable currency code to one token contract.
///
/// Precision and name belong to the associated [`Currency`]. Endpoint settings are not persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCurrency {
    pub chain_id: u32,
    pub address: String,
    pub symbol: String,
    pub chain_name: Blockchain,
}

impl TokenCurrency {
    /// Returns the currency code as `SYMBOL@CHAIN`.
    #[must_use]
    pub fn code(&self) -> String {
        format!(
            "{}@{}",
            self.symbol,
            self.chain_name.to_string().to_uppercase()
        )
    }

    /// Checks the contract address and its associated currency definition.
    ///
    /// # Errors
    ///
    /// Returns an error if the address is noncanonical, the code mismatches, the symbol is invalid,
    /// or the type is not a token.
    pub fn validate(&self, currency: Currency) -> CorrectnessResult<()> {
        let address = Address::from_str(&self.address).map_err(|_| conflict(currency.code))?;
        if self.address != format!("{address:#x}")
            || currency.code.as_str() != self.code()
            || currency.iso4217 != 0
            || currency.currency_type != CurrencyType::Crypto
            || self.symbol.contains([':', '@'])
            || self.symbol.chars().any(char::is_whitespace)
        {
            return Err(conflict(currency.code));
        }

        check_valid_string_utf8(&self.symbol, "symbol")
    }
}

impl From<&Token> for TokenCurrency {
    fn from(token: &Token) -> Self {
        Self {
            chain_id: token.chain.chain_id,
            address: format!("{:#x}", token.address),
            symbol: token.symbol.clone(),
            chain_name: token.chain.name,
        }
    }
}

/// Registers a token code and its chain/contract binding, retaining the first name and symbol.
///
/// # Errors
///
/// Returns an error if metadata is invalid, a legacy code exists, or an amount definition conflicts.
pub fn register(currency: Currency, metadata: TokenCurrency) -> CorrectnessResult<Currency> {
    Currency::new_checked(
        currency.code.as_str(),
        currency.precision,
        currency.iso4217,
        currency.name.as_str(),
        currency.currency_type,
    )?;
    metadata.validate(currency)?;
    let mut currencies = CURRENCY_MAP.lock();
    let mut tokens = TOKEN_CURRENCIES.lock();
    if let Some((registered, existing)) = tokens.get(&currency.code) {
        check_definition(*registered, currency)?;
        if existing.chain_id != metadata.chain_id || existing.address != metadata.address {
            return Err(conflict(currency.code));
        }

        if existing != &metadata || registered.name != currency.name {
            log::warn!(
                "Token {} display metadata differs from the registered definition; keeping the registered metadata",
                currency.code,
            );
        }

        return Ok(*registered);
    }

    if let Some((registered, _)) = tokens.values().find(|(_, existing)| {
        existing.chain_id == metadata.chain_id && existing.address == metadata.address
    }) {
        check_definition(*registered, currency)?;
        log::warn!(
            "Token {} symbol differs from its registered definition; keeping the registered code {}",
            currency.code,
            registered.code
        );
        return Ok(*registered);
    }

    if currencies.contains_key(currency.code.as_str()) {
        return Err(conflict(currency.code));
    }

    currencies.insert(currency.code.to_string(), currency);
    tokens.insert(currency.code, (currency, metadata));
    Ok(currency)
}

/// Returns a registered token definition, without inferring provenance from its code.
#[must_use]
pub fn lookup(code: Ustr) -> Option<(Currency, TokenCurrency)> {
    if !code.contains('@') {
        return None;
    }

    TOKEN_CURRENCIES.lock().get(&code).cloned()
}

/// Checks a currency against its registered token definition.
///
/// # Errors
///
/// Returns an error if a registered token has a conflicting amount definition.
pub fn check(currency: Currency) -> CorrectnessResult<Option<TokenCurrency>> {
    if !currency.code.contains('@') {
        return Ok(None);
    }

    let tokens = TOKEN_CURRENCIES.lock();

    let Some((registered, metadata)) = tokens.get(&currency.code) else {
        return Ok(None);
    };

    check_definition(*registered, currency)?;
    Ok(Some(metadata.clone()))
}

/// Returns the sorted, distinct token currency codes used by an instrument.
///
/// # Errors
///
/// Returns an error if an instrument carries a conflicting token definition.
pub fn instrument_codes(instrument: &dyn Instrument) -> CorrectnessResult<Vec<String>> {
    let mut codes = Vec::new();

    for currency in instrument.base_currency().into_iter().chain([
        instrument.quote_currency(),
        instrument.settlement_currency(),
    ]) {
        if check(currency)?.is_some() {
            codes.push(currency.code.to_string());
        }
    }

    codes.sort_unstable();
    codes.dedup();
    Ok(codes)
}

pub(crate) fn check_registration(currency: Currency) -> CorrectnessResult<Option<Currency>> {
    if !currency.code.contains('@') {
        return Ok(None);
    }

    let tokens = TOKEN_CURRENCIES.lock();

    let Some((registered, _)) = tokens.get(&currency.code) else {
        return Ok(None);
    };

    check_definition(*registered, currency)?;
    if registered.name != currency.name {
        log::warn!(
            "Token {} name differs from the registered definition; keeping the registered name",
            currency.code,
        );
    }

    Ok(Some(*registered))
}

fn check_definition(registered: Currency, currency: Currency) -> CorrectnessResult<()> {
    if registered.precision != currency.precision
        || registered.iso4217 != currency.iso4217
        || registered.currency_type != currency.currency_type
    {
        return Err(conflict(currency.code));
    }

    Ok(())
}

fn conflict(code: Ustr) -> CorrectnessError {
    CorrectnessError::PredicateViolation {
        message: format!("Conflicting DeFi currency definition for {code}"),
    }
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, sync::Arc};

    use alloy_primitives::{Address, U256};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{defi::Chain, types::Money};

    #[rstest]
    fn test_contract_and_chain_identity() {
        let first = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_301)),
            Address::from([11; 20]),
            "First".into(),
            "IDENTITY".into(),
            18,
        );
        let mut conflicting = first.clone();
        conflicting.address = Address::from([12; 20]);
        let mut other_chain = first.clone();
        other_chain.chain = Arc::new(Chain::new(Blockchain::Base, 812_302));

        let first_currency = first.currency().unwrap();
        let conflict = conflicting.currency().unwrap_err();
        let third_currency = other_chain.currency().unwrap();
        let codes = HashSet::from([first_currency, third_currency]);

        assert_eq!(codes.len(), 2);
        assert_eq!(first_currency.code, "IDENTITY@ETHEREUM");
        assert_eq!(third_currency.code, "IDENTITY@BASE");
        assert_eq!(conflict, super::conflict(first_currency.code));
        assert_eq!(
            lookup(first_currency.code).unwrap().1,
            TokenCurrency {
                chain_id: 812_301,
                address: format!("{:#x}", first.address),
                symbol: "IDENTITY".into(),
                chain_name: Blockchain::Ethereum,
            }
        );
        assert!(
            std::panic::catch_unwind(
                || Money::zero(first_currency).checked_add(Money::zero(third_currency))
            )
            .is_err()
        );
    }

    #[rstest]
    #[case(17, "0.00000000000000001")]
    #[case(18, "0.000000000000000001")]
    fn test_exact_token_amount(#[case] precision: u8, #[case] amount: &str) {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_303)),
            Address::from([precision; 20]),
            "Exact".into(),
            format!("EXACT{precision}"),
            precision,
        );
        let currency = token.currency().unwrap();

        let money = Money::from_u256(U256::from(1), currency).unwrap();
        let encoded = serde_json::to_vec(&money).unwrap();
        let restored: Money = serde_json::from_slice(&encoded).unwrap();

        assert_eq!(money.raw(), 1);
        assert_eq!(
            money.as_decimal(),
            amount.parse::<rust_decimal::Decimal>().unwrap()
        );
        assert_eq!(restored, money);
        assert_eq!(restored.currency.precision, precision);
        assert_eq!(restored.as_decimal(), money.as_decimal());
    }

    #[rstest]
    #[case(812_312, [27; 20], [28; 20])]
    #[case(812_313, [30; 20], [30; 20])]
    fn test_code_rejects_conflicting_binding(
        #[case] chain_id: u32,
        #[case] original_address: [u8; 20],
        #[case] address: [u8; 20],
    ) {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_312)),
            Address::from(original_address),
            "Binding".into(),
            format!("BIND{chain_id}"),
            6,
        );
        let registered = token.currency().unwrap();
        let mut conflicting = token.clone();
        conflicting.chain = Arc::new(Chain::new(Blockchain::Ethereum, chain_id));
        conflicting.address = Address::from(address);

        let error = conflicting.currency().unwrap_err();

        assert_eq!(error, conflict(registered.code));
        assert_eq!(
            lookup(registered.code).unwrap().1,
            TokenCurrency::from(&token)
        );
    }

    #[rstest]
    #[case("BAD@TOKEN")]
    #[case("BAD:TOKEN")]
    #[case("BAD TOKEN")]
    #[case("BAD\nTOKEN")]
    fn test_code_rejects_invalid_symbol(#[case] symbol: &str) {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_314)),
            Address::from([29; 20]),
            "Invalid symbol".into(),
            symbol.into(),
            6,
        );
        let code = TokenCurrency::from(&token).code();

        let error = token.currency().unwrap_err();

        assert_eq!(error, conflict(Ustr::from(&code)));
        assert_eq!(lookup(Ustr::from(&code)), None);
    }

    #[rstest]
    fn test_token_rejects_unrepresentable_precision() {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_315)),
            Address::from([31; 20]),
            "Unsupported precision".into(),
            "DECIMALS19".into(),
            19,
        );
        let code = Ustr::from(&TokenCurrency::from(&token).code());

        let result = token.currency();

        assert!(result.is_err());
        assert_eq!(lookup(code), None);
        assert_eq!(Currency::try_from_str(code.as_str()), None);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_registration_conflict_in_both_orders(#[case] reversed: bool) {
        let chain_id = if reversed { 812_304 } else { 812_305 };

        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, chain_id)),
            Address::from([21; 20]),
            "Conflict".into(),
            format!("CONFLICT{chain_id}"),
            6,
        );
        let metadata = TokenCurrency::from(&token);
        let code = metadata.code();

        let first_precision = if reversed { 18 } else { 6 };
        let second_precision = if reversed { 6 } else { 18 };
        let first = Currency::new(&code, first_precision, 0, &token.name, CurrencyType::Crypto);

        let second = Currency::new(
            &code,
            second_precision,
            0,
            &token.name,
            CurrencyType::Crypto,
        );

        let registered = register(first, metadata.clone()).unwrap();
        let conflict = register(second, metadata).unwrap_err();
        let overwrite = Currency::register(second, true).unwrap_err();

        assert_eq!(registered.precision, first_precision);
        assert_eq!(conflict, overwrite);
        assert_eq!(
            Currency::try_from_str(&code).unwrap().precision,
            first_precision
        );
    }

    #[rstest]
    fn test_display_changes_retain_first_definition() {
        let mut token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_306)),
            Address::from([22; 20]),
            "Original".into(),
            "OLD".into(),
            8,
        );
        let original_metadata = TokenCurrency::from(&token);
        let first = token.currency().unwrap();
        token.name = "Renamed".into();
        token.symbol = "NEW".into();

        let renamed = token.currency().unwrap();
        let (_, metadata) = lookup(first.code).unwrap();

        assert_eq!(renamed.code, first.code);
        assert_eq!(metadata, original_metadata);
        assert_eq!(renamed.name, "Original");
        assert_eq!(renamed.precision, 8);
        assert_eq!(metadata.symbol, "OLD");
        assert_eq!(metadata.code(), "OLD@ETHEREUM");
        assert_eq!(
            Money::from_decimal(dec!(0.12345678), renamed)
                .unwrap()
                .as_decimal(),
            dec!(0.12345678)
        );
    }

    #[rstest]
    fn test_legacy_code_is_not_upgraded() {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_307)),
            Address::from([23; 20]),
            "Legacy".into(),
            "LEGACY".into(),
            6,
        );
        let metadata = TokenCurrency::from(&token);
        let code = metadata.code();
        let legacy = Currency::new(&code, 6, 0, &token.name, CurrencyType::Crypto);
        Currency::register(legacy, false).unwrap();

        let result = token.currency();

        assert!(result.is_err());
        assert_eq!(lookup(legacy.code), None);
        assert_eq!(Currency::try_from_str(&code).unwrap().name, "Legacy");
    }

    #[rstest]
    #[case(19, "Valid")]
    #[case(6, "")]
    fn test_registration_rejects_invalid_public_fields(#[case] precision: u8, #[case] name: &str) {
        let chain_id = if name.is_empty() { 812_309 } else { 812_308 };

        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, chain_id)),
            Address::from([24; 20]),
            "Valid".into(),
            format!("INVALID{chain_id}"),
            6,
        );
        let metadata = TokenCurrency::from(&token);
        let code = metadata.code();
        let mut currency = Currency::new(code.as_str(), 6, 0, "Valid", CurrencyType::Crypto);
        currency.precision = precision;
        currency.name = Ustr::from(name);

        let result = register(currency, metadata);

        assert!(result.is_err());
        assert_eq!(lookup(currency.code), None);
        assert_eq!(Currency::try_from_str(&code), None);
    }

    #[rstest]
    fn test_get_or_create_uses_registered_token_definition() {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_310)),
            Address::from([25; 20]),
            "Six decimals".into(),
            "LOOKUP".into(),
            6,
        );
        let currency = token.currency().unwrap();

        let resolved = Currency::get_or_create_crypto(currency.code);

        assert_eq!(resolved.code, currency.code);
        assert_eq!(resolved.precision, 6);
        assert_eq!(resolved.iso4217, 0);
        assert_eq!(resolved.name, "Six decimals");
        assert_eq!(resolved.currency_type, CurrencyType::Crypto);
    }

    #[cfg(feature = "ffi")]
    #[rstest]
    fn test_ffi_cannot_overwrite_token_precision() {
        let token = Token::new(
            Arc::new(Chain::new(Blockchain::Ethereum, 812_311)),
            Address::from([26; 20]),
            "Guarded".into(),
            "FFIGUARD".into(),
            6,
        );
        let original = token.currency().unwrap();
        let mut conflicting = original;
        conflicting.precision = 18;

        crate::ffi::types::currency::currency_register(conflicting);

        assert_eq!(
            Currency::try_from_str(original.code.as_str())
                .unwrap()
                .precision,
            6
        );
    }
}
