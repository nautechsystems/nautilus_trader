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

//! Type-safe string wrappers for message bus patterns, topics, and endpoints.

use std::{fmt::Display, ops::Deref};

use nautilus_core::correctness::{FAILED, check_valid_string_utf8};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

/// Check that a string contains no wildcard characters.
#[inline(always)]
fn check_no_wildcards(value: &Ustr, key: &str) -> anyhow::Result<()> {
    // Check bytes directly - faster than chars() for ASCII wildcards
    if value.as_bytes().iter().any(|&b| b == b'*' || b == b'?') {
        anyhow::bail!("{key} `value` contained invalid characters, was {value}");
    }
    Ok(())
}

/// Marker for subscription patterns. Allows wildcards (`*`, `?`).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Pattern;

/// Marker for publish topics. No wildcards allowed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Topic;

/// Marker for direct message endpoints. No wildcards allowed.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub struct Endpoint;

/// A message bus string type parameterized by marker type.
///
/// - `MStr<Pattern>` - for subscriptions, allows wildcards (`*`, `?`)
/// - `MStr<Topic>` - for publishing, no wildcards
/// - `MStr<Endpoint>` - for direct messages, no wildcards
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MStr<T> {
    value: Ustr,
    #[serde(skip)]
    _marker: std::marker::PhantomData<T>,
}

impl<T> Display for MStr<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.value)
    }
}

impl<T> Deref for MStr<T> {
    type Target = Ustr;

    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> AsRef<str> for MStr<T> {
    fn as_ref(&self) -> &str {
        self.value.as_str()
    }
}

impl MStr<Pattern> {
    /// Create a new pattern from a string.
    pub fn pattern<T: AsRef<str>>(value: T) -> Self {
        let value = Ustr::from(value.as_ref());

        Self {
            value,
            _marker: std::marker::PhantomData,
        }
    }
}

impl From<&str> for MStr<Pattern> {
    fn from(value: &str) -> Self {
        Self::pattern(value)
    }
}

impl From<String> for MStr<Pattern> {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

impl From<&String> for MStr<Pattern> {
    fn from(value: &String) -> Self {
        value.as_str().into()
    }
}

impl From<MStr<Topic>> for MStr<Pattern> {
    fn from(value: MStr<Topic>) -> Self {
        Self {
            value: value.value,
            _marker: std::marker::PhantomData,
        }
    }
}

impl MStr<Topic> {
    /// Create a new topic from a fully qualified string.
    ///
    /// # Errors
    ///
    /// Returns an error if the topic has white space or invalid characters.
    pub fn topic<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let topic = Ustr::from(value.as_ref());
        check_valid_string_utf8(value, stringify!(value))?;
        check_no_wildcards(&topic, stringify!(Topic))?;

        Ok(Self {
            value: topic,
            _marker: std::marker::PhantomData,
        })
    }

    /// Create a topic from an already-interned Ustr.
    ///
    /// # Errors
    ///
    /// Returns an error if the topic is empty, all whitespace, or contains wildcard characters.
    pub fn topic_from_ustr(value: Ustr) -> anyhow::Result<Self> {
        check_valid_string_utf8(value.as_str(), stringify!(value))?;
        check_no_wildcards(&value, stringify!(Topic))?;

        Ok(Self {
            value,
            _marker: std::marker::PhantomData,
        })
    }
}

impl From<&str> for MStr<Topic> {
    fn from(value: &str) -> Self {
        Self::topic(value).expect(FAILED)
    }
}

impl From<String> for MStr<Topic> {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

impl From<&String> for MStr<Topic> {
    fn from(value: &String) -> Self {
        value.as_str().into()
    }
}

impl From<Ustr> for MStr<Topic> {
    fn from(value: Ustr) -> Self {
        Self::topic_from_ustr(value).expect(FAILED)
    }
}

impl From<&Ustr> for MStr<Topic> {
    fn from(value: &Ustr) -> Self {
        (*value).into()
    }
}

impl MStr<Endpoint> {
    /// Create a new endpoint from a fully qualified string.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint has white space or invalid characters.
    pub fn endpoint<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let endpoint = Ustr::from(value.as_ref());
        check_valid_string_utf8(value, stringify!(value))?;
        check_no_wildcards(&endpoint, stringify!(Endpoint))?;

        Ok(Self {
            value: endpoint,
            _marker: std::marker::PhantomData,
        })
    }

    /// Create an endpoint from an already-interned Ustr.
    ///
    /// # Errors
    ///
    /// Returns an error if the endpoint is empty, all whitespace, or contains wildcard characters.
    pub fn endpoint_from_ustr(value: Ustr) -> anyhow::Result<Self> {
        check_valid_string_utf8(value.as_str(), stringify!(value))?;
        check_no_wildcards(&value, stringify!(Endpoint))?;

        Ok(Self {
            value,
            _marker: std::marker::PhantomData,
        })
    }
}

impl From<&str> for MStr<Endpoint> {
    fn from(value: &str) -> Self {
        Self::endpoint(value).expect(FAILED)
    }
}

impl From<String> for MStr<Endpoint> {
    fn from(value: String) -> Self {
        value.as_str().into()
    }
}

impl From<&String> for MStr<Endpoint> {
    fn from(value: &String) -> Self {
        value.as_str().into()
    }
}

impl From<Ustr> for MStr<Endpoint> {
    fn from(value: Ustr) -> Self {
        Self::endpoint_from_ustr(value).expect(FAILED)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("data.quotes.BINANCE.BTCUSDT")]
    #[case("events.order.filled")]
    #[case("a")]
    #[case("a.b.c.d.e.f")]
    fn test_topic_valid(#[case] input: &str) {
        let topic = MStr::<Topic>::topic(input).unwrap();
        assert_eq!(topic.as_ref(), input);
    }

    #[rstest]
    #[case("data.*.BINANCE")]
    #[case("events.order.*")]
    #[case("*")]
    #[case("data.quotes.?")]
    #[case("a?b")]
    fn test_topic_rejects_wildcards(#[case] input: &str) {
        assert!(MStr::<Topic>::topic(input).is_err());
    }

    #[rstest]
    #[case("DataEngine.execute")]
    #[case("RiskEngine.process")]
    fn test_endpoint_valid(#[case] input: &str) {
        let endpoint = MStr::<Endpoint>::endpoint(input).unwrap();
        assert_eq!(endpoint.as_ref(), input);
    }

    #[rstest]
    #[case("DataEngine.*")]
    #[case("*.execute")]
    #[case("Risk?Engine")]
    fn test_endpoint_rejects_wildcards(#[case] input: &str) {
        assert!(MStr::<Endpoint>::endpoint(input).is_err());
    }

    #[rstest]
    #[case("data.*")]
    #[case("*.quotes.*")]
    #[case("data.?.BINANCE")]
    #[case("*")]
    #[case("exact.match.no.wildcards")]
    fn test_pattern_accepts_all(#[case] input: &str) {
        let pattern = MStr::<Pattern>::pattern(input);
        assert_eq!(pattern.as_ref(), input);
    }

    #[rstest]
    fn test_topic_to_pattern_conversion() {
        let topic: MStr<Topic> = "data.quotes.BINANCE.BTCUSDT".into();
        let pattern: MStr<Pattern> = topic.into();
        assert_eq!(pattern.as_ref(), "data.quotes.BINANCE.BTCUSDT");
    }

    #[rstest]
    fn test_topic_from_ustr_valid() {
        let ustr = Ustr::from("data.quotes.BINANCE");
        let topic = MStr::<Topic>::topic_from_ustr(ustr).unwrap();
        assert_eq!(topic.as_ref(), "data.quotes.BINANCE");
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    #[case("\t\n")]
    fn test_topic_from_ustr_rejects_empty_whitespace(#[case] input: &str) {
        let ustr = Ustr::from(input);
        assert!(MStr::<Topic>::topic_from_ustr(ustr).is_err());
    }

    #[rstest]
    #[case("data.*")]
    #[case("a?b")]
    fn test_topic_from_ustr_rejects_wildcards(#[case] input: &str) {
        let ustr = Ustr::from(input);
        assert!(MStr::<Topic>::topic_from_ustr(ustr).is_err());
    }

    #[rstest]
    fn test_endpoint_from_ustr_valid() {
        let ustr = Ustr::from("DataEngine.execute");
        let endpoint = MStr::<Endpoint>::endpoint_from_ustr(ustr).unwrap();
        assert_eq!(endpoint.as_ref(), "DataEngine.execute");
    }

    #[rstest]
    #[case("")]
    #[case("   ")]
    fn test_endpoint_from_ustr_rejects_empty_whitespace(#[case] input: &str) {
        let ustr = Ustr::from(input);
        assert!(MStr::<Endpoint>::endpoint_from_ustr(ustr).is_err());
    }

    #[rstest]
    #[case("Engine.*")]
    #[case("a?b")]
    fn test_endpoint_from_ustr_rejects_wildcards(#[case] input: &str) {
        let ustr = Ustr::from(input);
        assert!(MStr::<Endpoint>::endpoint_from_ustr(ustr).is_err());
    }

    #[rstest]
    fn test_from_impls_equivalent() {
        let s = "test.topic";
        let from_str: MStr<Topic> = s.into();
        let from_string: MStr<Topic> = s.to_string().into();
        let from_string_ref: MStr<Topic> = (&s.to_string()).into();
        let from_ustr: MStr<Topic> = Ustr::from(s).into();

        assert_eq!(from_str, from_string);
        assert_eq!(from_string, from_string_ref);
        assert_eq!(from_string_ref, from_ustr);
    }

    #[rstest]
    fn test_deref_to_ustr() {
        let topic: MStr<Topic> = "test.topic".into();
        let ustr: &Ustr = &topic;
        assert_eq!(ustr.as_str(), "test.topic");
    }

    fn valid_segment() -> impl Strategy<Value = String> {
        "[a-zA-Z][a-zA-Z0-9_]{0,15}".prop_filter("non-empty", |s| !s.is_empty())
    }

    fn valid_topic_string() -> impl Strategy<Value = String> {
        prop::collection::vec(valid_segment(), 1..=5).prop_map(|segs| segs.join("."))
    }

    fn string_with_wildcards() -> impl Strategy<Value = String> {
        prop::collection::vec(
            prop_oneof![
                valid_segment(),
                Just("*".to_string()),
                Just("?".to_string()),
            ],
            1..=5,
        )
        .prop_map(|segs| segs.join("."))
        .prop_filter("must contain wildcard", |s| {
            s.contains('*') || s.contains('?')
        })
    }

    proptest! {
        #[rstest]
        fn prop_topic_roundtrip(s in valid_topic_string()) {
            let topic = MStr::<Topic>::topic(&s).unwrap();
            prop_assert_eq!(topic.as_ref(), s.as_str());
        }

        #[rstest]
        fn prop_endpoint_roundtrip(s in valid_topic_string()) {
            let endpoint = MStr::<Endpoint>::endpoint(&s).unwrap();
            prop_assert_eq!(endpoint.as_ref(), s.as_str());
        }

        #[rstest]
        fn prop_pattern_accepts_wildcards(s in string_with_wildcards()) {
            let pattern = MStr::<Pattern>::pattern(&s);
            prop_assert_eq!(pattern.as_ref(), s.as_str());
        }

        #[rstest]
        fn prop_topic_rejects_wildcards(s in string_with_wildcards()) {
            prop_assert!(MStr::<Topic>::topic(&s).is_err());
        }

        #[rstest]
        fn prop_endpoint_rejects_wildcards(s in string_with_wildcards()) {
            prop_assert!(MStr::<Endpoint>::endpoint(&s).is_err());
        }

        #[rstest]
        fn prop_topic_to_pattern_preserves_value(s in valid_topic_string()) {
            let topic: MStr<Topic> = MStr::topic(&s).unwrap();
            let pattern: MStr<Pattern> = topic.into();
            prop_assert_eq!(pattern.as_ref(), s.as_str());
        }

        #[rstest]
        fn prop_from_impls_consistent(s in valid_topic_string()) {
            let from_str: MStr<Topic> = s.as_str().into();
            let from_string: MStr<Topic> = s.clone().into();
            let from_ustr: MStr<Topic> = Ustr::from(&s).into();

            prop_assert_eq!(from_str, from_string);
            prop_assert_eq!(from_string, from_ustr);
        }
    }
}
