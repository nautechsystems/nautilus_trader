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

//! Tests for currency handling with newly listed assets (issue #3898)

use std::str::FromStr;
use nautilus_model::{
    enums::CurrencyType,
    types::Currency,
};

#[test]
fn test_get_or_create_crypto_with_newly_listed_currency() {
    let currency = Currency::get_or_create_crypto("0G");

    assert_eq!(currency.code.as_str(), "0G");
    assert_eq!(currency.precision, 8);
    assert_eq!(currency.iso4217, 0);
    assert_eq!(currency.name.as_str(), "0G");
    assert_eq!(currency.currency_type, CurrencyType::Crypto);
    
    println!("Successfully created newly listed currency '0G'");
}

#[test]
fn test_get_or_create_crypto_is_idempotent() {
    let currency1 = Currency::get_or_create_crypto("NEWCOIN");
    let currency2 = Currency::get_or_create_crypto("NEWCOIN");
    
    assert_eq!(currency1, currency2);
    println!("get_or_create_crypto is idempotent");
}

#[test]
fn test_get_or_create_crypto_with_existing_currency() {
    let btc = Currency::get_or_create_crypto("BTC");
    assert_eq!(btc.code.as_str(), "BTC");
    assert_eq!(btc.precision, 8);
    
    println!("get_or_create_crypto works with existing currencies");
}

#[test]
fn test_try_from_str_finds_newly_created_currency() {
    let code = "TESTCOIN123";
    
    let created = Currency::get_or_create_crypto(code);
    let found = Currency::try_from_str(code);
    
    assert!(found.is_some());
    assert_eq!(found.unwrap(), created);
    
    println!("Newly created currency is properly registered");
}

#[test]
fn test_demonstrate_from_str_vs_get_or_create_difference() {

    let unknown_currency = "UNKNOWN_NEW_ASSET";
    
    let from_str_result = Currency::from_str(unknown_currency);
    assert!(
        from_str_result.is_err(),
        "Currency::from_str should fail for unknown currency"
    );
    println!(
        "Currency::from_str('{}') failed: {}",
        unknown_currency,
        from_str_result.unwrap_err()
    );

    let get_or_create_result = Currency::get_or_create_crypto(unknown_currency);
    assert_eq!(get_or_create_result.code.as_str(), unknown_currency);
    assert_eq!(get_or_create_result.currency_type, CurrencyType::Crypto);
    println!(
        "Currency::get_or_create_crypto('{}') succeeded: code='{}', precision={}, type=CRYPTO",
        unknown_currency, get_or_create_result.code, get_or_create_result.precision
    );
    
    // Verify it's now registered and can be found
    let found = Currency::try_from_str(unknown_currency);
    assert!(found.is_some());
    assert_eq!(found.unwrap(), get_or_create_result);
    
    println!("Newly created currency is registered and findable");
}

#[test]
fn test_multiple_newly_listed_currencies() {
    // Test that we can handle multiple newly listed currencies
    let currencies = vec!["0G", "1INCH", "DOGE", "SHIB"];
    
    for code in currencies {
        let currency = Currency::get_or_create_crypto(code);
        assert_eq!(currency.code.as_str(), code);
        assert_eq!(currency.currency_type, CurrencyType::Crypto);
        
        // Verify it's findable
        let found = Currency::try_from_str(code);
        assert!(found.is_some());
    }
    
    println!("Successfully created and registered multiple newly listed currencies");
}