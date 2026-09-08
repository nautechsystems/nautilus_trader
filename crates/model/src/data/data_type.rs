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

use std::{
    collections::HashMap,
    fmt::{Debug, Display, Write as _},
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{Params, UnixNanos};
use serde::{
    Deserialize, Serialize,
    de::{self, IgnoredAny, MapAccess, SeqAccess, Visitor},
};
use serde_json::Value as JsonValue;

use crate::identifiers::{InstrumentId, Venue};

/// Separates a custom data topic from its identifier.
pub const IDENTIFIER_TOPIC_SUFFIX: &str = ".identifier=";

/// Prefix reserved for metadata strings that would otherwise parse as JSON topic values.
const STRING_TOPIC_PREFIX: &str = "~s~";
const ESCAPED_STRING_TOPIC_PREFIX: &str = "~x~";

/// Builds a topic value while preserving the distinction between JSON strings and scalar values.
fn value_to_topic_string(v: &JsonValue) -> String {
    if let Some(s) = v.as_str() {
        let has_pair_shaped_segment = s.split('.').skip(1).any(|segment| segment.contains('='));
        if has_pair_shaped_segment || s.starts_with(ESCAPED_STRING_TOPIC_PREFIX) {
            let mut escaped =
                String::with_capacity(ESCAPED_STRING_TOPIC_PREFIX.len() + s.len() * 2);
            escaped.push_str(ESCAPED_STRING_TOPIC_PREFIX);

            for byte in s.as_bytes() {
                write!(escaped, "{byte:02x}").expect("writing to String cannot fail");
            }
            return escaped;
        }

        if serde_json::from_str::<JsonValue>(s).is_ok() || s.starts_with(STRING_TOPIC_PREFIX) {
            return format!("{STRING_TOPIC_PREFIX}{s}");
        }
        return s.to_string();
    }

    if let Some(n) = v.as_u64() {
        return n.to_string();
    }

    if let Some(n) = v.as_i64() {
        return n.to_string();
    }

    if let Some(b) = v.as_bool() {
        return b.to_string();
    }

    if let Some(f) = v.as_f64() {
        let value = f.to_string();
        return if value.contains('.') || value.contains('e') || value.contains('E') {
            value
        } else {
            format!("{value}.0")
        };
    }

    if v.is_null() {
        return "null".to_string();
    }
    serde_json::to_string(v).unwrap_or_default()
}

/// Decodes one reserved string prefix before attempting JSON topic-value parsing.
fn topic_string_to_value(s: &str) -> anyhow::Result<JsonValue> {
    if let Some(value) = s.strip_prefix(ESCAPED_STRING_TOPIC_PREFIX) {
        anyhow::ensure!(
            value.len().is_multiple_of(2),
            "Invalid escaped topic string"
        );
        let bytes = (0..value.len())
            .step_by(2)
            .map(|index| u8::from_str_radix(&value[index..index + 2], 16))
            .collect::<Result<Vec<_>, _>>()?;
        return Ok(JsonValue::String(String::from_utf8(bytes)?));
    }

    if let Some(value) = s.strip_prefix(STRING_TOPIC_PREFIX) {
        return Ok(JsonValue::String(value.to_string()));
    }

    Ok(serde_json::from_str(s).unwrap_or_else(|_| JsonValue::String(s.to_string())))
}

fn parse_topic_metadata(metadata_topic: &str) -> anyhow::Result<Params> {
    let mut metadata = Params::new();
    if metadata_topic.is_empty() {
        return Ok(metadata);
    }

    // Values may contain dots: a segment with '=' starts a new pair, otherwise it
    // extends the current value (or the first key before any pair has started).
    let mut pending_key: Option<String> = None;
    let mut current: Option<(String, String)> = None;

    for segment in metadata_topic.split('.') {
        if let Some((key, value)) = segment.split_once('=') {
            let key = if let Some(prefix) = pending_key.take() {
                format!("{prefix}.{key}")
            } else {
                if key.is_empty() {
                    anyhow::bail!("Invalid empty metadata topic key");
                }
                key.to_string()
            };

            if let Some((prev_key, prev_value)) = current.replace((key, value.to_string())) {
                metadata.insert(prev_key, topic_string_to_value(&prev_value)?);
            }
        } else if let Some((_, value)) = current.as_mut() {
            value.push('.');
            value.push_str(segment);
        } else if let Some(prefix) = pending_key.as_mut() {
            prefix.push('.');
            prefix.push_str(segment);
        } else {
            pending_key = Some(segment.to_string());
        }
    }

    let Some((key, value)) = current else {
        anyhow::bail!("Invalid metadata topic pair: {metadata_topic}");
    };
    metadata.insert(key, topic_string_to_value(&value)?);

    Ok(metadata)
}

/// Builds the topic suffix from Params (string-only view: key=value joined by ".").
fn params_to_topic_suffix(params: &Params) -> String {
    let mut entries = params.iter().collect::<Vec<_>>();
    entries.sort_by_key(|(key, _)| *key);

    entries
        .into_iter()
        .map(|(k, v)| format!("{k}={}", value_to_topic_string(v)))
        .collect::<Vec<_>>()
        .join(".")
}

/// Represents a data type including metadata.
#[derive(Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct DataType {
    type_name: String,
    metadata: Option<Params>,
    topic: String,
    hash: u64,
    identifier: Option<String>,
}

impl<'de> Deserialize<'de> for DataType {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        const FIELDS: &[&str] = &["type_name", "metadata", "topic", "hash", "identifier"];

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "snake_case")]
        enum Field {
            TypeName,
            Metadata,
            Topic,
            Hash,
            Identifier,
            #[serde(other)]
            Other,
        }

        fn finish(
            type_name: &str,
            metadata: Option<Params>,
            topic: Option<String>,
            identifier: Option<String>,
        ) -> anyhow::Result<DataType> {
            let mut data_type = DataType::try_new(type_name, metadata, identifier)?;
            if let Some(topic) = topic {
                data_type.hash = calculate_hash(&topic);
                data_type.topic = topic;
            }
            Ok(data_type)
        }

        struct DataTypeVisitor;

        impl<'de> Visitor<'de> for DataTypeVisitor {
            type Value = DataType;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("struct DataType")
            }

            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: SeqAccess<'de>,
            {
                let type_name: String = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(0, &self))?;
                let metadata = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(1, &self))?;
                let topic = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(2, &self))?;
                let _hash: u64 = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(3, &self))?;
                let identifier = seq
                    .next_element()?
                    .ok_or_else(|| de::Error::invalid_length(4, &self))?;

                finish(&type_name, metadata, Some(topic), identifier).map_err(de::Error::custom)
            }

            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: MapAccess<'de>,
            {
                let mut type_name: Option<String> = None;
                let mut metadata: Option<Option<Params>> = None;
                let mut topic: Option<Option<String>> = None;
                let mut hash_seen = false;
                let mut identifier: Option<Option<String>> = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::TypeName => {
                            if type_name.is_some() {
                                return Err(de::Error::duplicate_field("type_name"));
                            }
                            type_name = Some(map.next_value()?);
                        }
                        Field::Metadata => {
                            if metadata.is_some() {
                                return Err(de::Error::duplicate_field("metadata"));
                            }
                            metadata = Some(map.next_value()?);
                        }
                        Field::Topic => {
                            if topic.is_some() {
                                return Err(de::Error::duplicate_field("topic"));
                            }
                            topic = Some(map.next_value()?);
                        }
                        Field::Hash => {
                            if hash_seen {
                                return Err(de::Error::duplicate_field("hash"));
                            }
                            hash_seen = true;
                            let _: Option<u64> = map.next_value()?;
                        }
                        Field::Identifier => {
                            if identifier.is_some() {
                                return Err(de::Error::duplicate_field("identifier"));
                            }
                            identifier = Some(map.next_value()?);
                        }
                        Field::Other => {
                            let _: IgnoredAny = map.next_value()?;
                        }
                    }
                }

                let type_name = type_name.ok_or_else(|| de::Error::missing_field("type_name"))?;
                finish(
                    &type_name,
                    metadata.unwrap_or(None),
                    topic.unwrap_or(None),
                    identifier.unwrap_or(None),
                )
                .map_err(de::Error::custom)
            }
        }

        deserializer.deserialize_struct("DataType", FIELDS, DataTypeVisitor)
    }
}

fn calculate_hash(topic: &str) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    topic.hash(&mut hasher);
    hasher.finish()
}

impl DataType {
    /// Creates a new [`DataType`] instance.
    ///
    /// # Panics
    ///
    /// Panics if `metadata` contains the reserved `identifier` key.
    #[must_use]
    pub fn new(type_name: &str, metadata: Option<Params>, identifier: Option<String>) -> Self {
        Self::try_new(type_name, metadata, identifier)
            .expect("DataType metadata must not contain the reserved `identifier` key")
    }

    /// Creates a new [`DataType`] instance after validating reserved metadata keys.
    ///
    /// # Errors
    ///
    /// Returns an error if `metadata` contains the reserved `identifier` key.
    pub fn try_new(
        type_name: &str,
        metadata: Option<Params>,
        identifier: Option<String>,
    ) -> anyhow::Result<Self> {
        anyhow::ensure!(
            metadata
                .as_ref()
                .is_none_or(|metadata| !metadata.contains_key("identifier")),
            "`identifier` is reserved for DataType catalog identity",
        );
        let base_topic = if let Some(ref meta) = metadata {
            if meta.is_empty() {
                type_name.to_string()
            } else {
                format!("{type_name}.{}", params_to_topic_suffix(meta))
            }
        } else {
            type_name.to_string()
        };
        let topic = identifier
            .as_ref()
            .map_or(base_topic.clone(), |identifier| {
                format!("{base_topic}{IDENTIFIER_TOPIC_SUFFIX}{identifier}")
            });

        let hash = calculate_hash(&topic);

        Ok(Self {
            type_name: type_name.to_owned(),
            metadata,
            topic,
            hash,
            identifier,
        })
    }

    /// Serializes to JSON for persistence (`type_name`, metadata, identifier; no topic, no hash).
    ///
    /// # Errors
    ///
    /// Returns a JSON serialization error if the data cannot be serialized.
    pub fn to_persistence_json(&self) -> Result<String, serde_json::Error> {
        let mut map = serde_json::Map::new();
        map.insert(
            "type_name".to_string(),
            serde_json::Value::String(self.type_name.clone()),
        );
        map.insert(
            "metadata".to_string(),
            self.metadata.as_ref().map_or(serde_json::Value::Null, |m| {
                serde_json::to_value(m).unwrap_or(serde_json::Value::Null)
            }),
        );

        if let Some(ref id) = self.identifier {
            map.insert(
                "identifier".to_string(),
                serde_json::Value::String(id.clone()),
            );
        }
        serde_json::to_string(&serde_json::Value::Object(map))
    }

    /// Deserializes from JSON produced by `to_persistence_json`.
    /// Accepts legacy JSON with `topic` (ignored); topic is rebuilt from `type_name` + metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid JSON or missing required fields.
    pub fn from_persistence_json(s: &str) -> Result<Self, anyhow::Error> {
        let value: serde_json::Value =
            serde_json::from_str(s).map_err(|e| anyhow::anyhow!("Invalid data_type JSON: {e}"))?;
        let obj = value
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("data_type must be a JSON object"))?;
        let type_name = obj
            .get("type_name")
            .and_then(|v| v.as_str())
            .ok_or_else(|| anyhow::anyhow!("data_type must have type_name"))?
            .to_string();
        let metadata = obj.get("metadata").and_then(|m| {
            if m.is_null() {
                None
            } else {
                let p: Params = serde_json::from_value(m.clone()).ok()?;
                if p.is_empty() { None } else { Some(p) }
            }
        });
        let identifier = obj
            .get("identifier")
            .and_then(|v| v.as_str())
            .map(String::from);
        Self::try_new(&type_name, metadata, identifier)
    }

    /// Returns the type name for the data type.
    #[must_use]
    pub fn type_name(&self) -> &str {
        self.type_name.as_str()
    }

    /// Returns the metadata for the data type.
    #[must_use]
    pub fn metadata(&self) -> Option<&Params> {
        self.metadata.as_ref()
    }

    /// Returns a string representation of the metadata.
    #[must_use]
    pub fn metadata_str(&self) -> String {
        self.metadata.as_ref().map_or_else(
            || "null".to_string(),
            |metadata| {
                let mut entries = metadata.iter().collect::<Vec<_>>();
                entries.sort_by_key(|(key, _)| *key);

                let mut metadata_map = serde_json::Map::new();
                for (key, value) in entries {
                    metadata_map.insert(key.clone(), value.clone());
                }

                serde_json::to_string(&metadata_map).unwrap_or_default()
            },
        )
    }

    /// Returns metadata as a string-only map (e.g. for Arrow schema metadata).
    #[must_use]
    pub fn metadata_string_map(&self) -> Option<HashMap<String, String>> {
        self.metadata.as_ref().map(|p| {
            p.iter()
                .map(|(k, v)| (k.clone(), value_to_topic_string(v)))
                .collect()
        })
    }

    /// Returns the precomputed hash for this data type.
    #[must_use]
    pub fn precomputed_hash(&self) -> u64 {
        self.hash
    }

    /// Returns the messaging topic for the data type.
    #[must_use]
    pub fn topic(&self) -> &str {
        self.topic.as_str()
    }

    /// Returns the optional catalog path identifier.
    #[must_use]
    pub fn identifier(&self) -> Option<&str> {
        self.identifier.as_deref()
    }

    /// Returns an optional [`InstrumentId`] parsed from the metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the `instrument_id` metadata value is invalid.
    pub fn instrument_id(&self) -> anyhow::Result<Option<InstrumentId>> {
        let Some(instrument_id) = self
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get_str("instrument_id"))
        else {
            return Ok(None);
        };

        InstrumentId::from_str(instrument_id)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("Invalid instrument_id metadata `{instrument_id}`: {e}"))
    }

    /// Returns an optional [`Venue`] parsed from the metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the `venue` metadata value is invalid.
    pub fn venue(&self) -> anyhow::Result<Option<Venue>> {
        let Some(venue) = self
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get_str("venue"))
        else {
            return Ok(None);
        };

        Venue::new_checked(venue)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("Invalid venue metadata `{venue}`: {e}"))
    }

    /// Returns an optional [`UnixNanos`] parsed from the metadata `start` field.
    ///
    /// # Errors
    ///
    /// Returns an error if the `start` metadata value is invalid.
    pub fn start(&self) -> anyhow::Result<Option<UnixNanos>> {
        self.parse_unix_nanos("start")
    }

    /// Returns an optional [`UnixNanos`] parsed from the metadata `end` field.
    ///
    /// # Errors
    ///
    /// Returns an error if the `end` metadata value is invalid.
    pub fn end(&self) -> anyhow::Result<Option<UnixNanos>> {
        self.parse_unix_nanos("end")
    }

    /// Returns an optional row limit parsed from the metadata.
    ///
    /// # Errors
    ///
    /// Returns an error if the `limit` metadata value is invalid.
    pub fn limit(&self) -> anyhow::Result<Option<usize>> {
        let Some(metadata) = self.metadata.as_ref() else {
            return Ok(None);
        };

        if let Some(limit) = metadata.get_usize("limit") {
            return Ok(Some(limit));
        }
        let Some(limit) = metadata.get_str("limit") else {
            return Ok(None);
        };

        limit
            .parse::<usize>()
            .map(Some)
            .map_err(|e| anyhow::anyhow!("Invalid limit metadata `{limit}`: {e}"))
    }

    fn parse_unix_nanos(&self, key: &str) -> anyhow::Result<Option<UnixNanos>> {
        let Some(value) = self
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.get_str(key))
        else {
            return Ok(None);
        };

        UnixNanos::from_str(value)
            .map(Some)
            .map_err(|e| anyhow::anyhow!("Invalid {key} metadata `{value}`: {e}"))
    }
}

impl PartialEq for DataType {
    fn eq(&self, other: &Self) -> bool {
        self.topic == other.topic
    }
}

impl Eq for DataType {}

impl PartialOrd for DataType {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for DataType {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.topic.cmp(&other.topic)
    }
}

impl Hash for DataType {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.hash.hash(state);
    }
}

impl FromStr for DataType {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (base_topic, identifier) = s
            .rsplit_once(IDENTIFIER_TOPIC_SUFFIX)
            .map_or((s, None), |(base, identifier)| {
                (base, Some(identifier.to_string()))
            });
        let (type_name, metadata) = match base_topic.split_once('.') {
            None => (base_topic, None),
            Some((type_name, metadata_topic)) => {
                let metadata = parse_topic_metadata(metadata_topic)?;
                (type_name, Some(metadata))
            }
        };

        Ok(Self::new(type_name, metadata, identifier))
    }
}

impl Display for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.topic)
    }
}

impl Debug for DataType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "DataType(type_name={}, metadata={:?}, identifier={:?})",
            self.type_name, self.metadata, self.identifier
        )
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::hash_map::DefaultHasher,
        hash::{Hash, Hasher},
    };

    use rstest::*;
    use serde_json::json;

    use super::*;

    fn params_from_json(value: serde_json::Value) -> Params {
        serde_json::from_value(value).expect("valid Params JSON")
    }

    #[rstest]
    fn test_data_type_creation_with_metadata() {
        let metadata = Some(params_from_json(
            json!({"key1": "value1", "key2": "value2"}),
        ));
        let data_type = DataType::new("ExampleType", metadata.clone(), None);

        assert_eq!(data_type.type_name(), "ExampleType");
        assert_eq!(data_type.topic(), "ExampleType.key1=value1.key2=value2");
        assert_eq!(data_type.metadata(), metadata.as_ref());
    }

    #[rstest]
    fn test_data_type_topic_roundtrip_escapes_ambiguous_metadata_string() {
        let metadata = Some(params_from_json(json!({"selector": "x.y=z"})));
        let data_type = DataType::new("ExampleType", metadata.clone(), None);

        let roundtrip = DataType::from_str(data_type.topic()).unwrap();

        assert_eq!(roundtrip.metadata(), metadata.as_ref());
        assert_eq!(roundtrip.topic(), data_type.topic());
    }

    #[rstest]
    fn test_data_type_topic_identity_uses_canonical_metadata_order() {
        let mut metadata1 = Params::new();
        metadata1.insert("b".to_string(), json!(2));
        metadata1.insert("a".to_string(), json!(1));
        let mut metadata2 = Params::new();
        metadata2.insert("a".to_string(), json!(1));
        metadata2.insert("b".to_string(), json!(2));

        let data_type1 = DataType::new("ExampleType", Some(metadata1), None);
        let data_type2 = DataType::new("ExampleType", Some(metadata2), None);
        let mut hasher1 = DefaultHasher::new();
        data_type1.hash(&mut hasher1);
        let hash1 = hasher1.finish();
        let mut hasher2 = DefaultHasher::new();
        data_type2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(data_type1.topic(), "ExampleType.a=1.b=2");
        assert_eq!(data_type1.topic(), data_type2.topic());
        assert_eq!(data_type1, data_type2);
        assert_eq!(hash1, hash2);
        assert_eq!(format!("{data_type1}"), format!("{data_type2}"));
        assert_eq!(data_type1.metadata_str(), r#"{"a":1,"b":2}"#);
        assert_eq!(data_type1.metadata_str(), data_type2.metadata_str());
    }

    #[rstest]
    fn test_data_type_deserialization_recomputes_hash_from_topic() {
        let payload = json!({
            "type_name": "ExampleType",
            "metadata": {"key": "value"},
            "topic": "custom.topic",
            "hash": calculate_hash("custom.topic") ^ u64::MAX,
            "identifier": "catalog/path",
        });

        let deserialized: DataType = serde_json::from_value(payload).unwrap();

        assert_eq!(deserialized.type_name(), "ExampleType");
        assert_eq!(
            deserialized.metadata(),
            Some(&params_from_json(json!({"key": "value"})))
        );
        assert_eq!(deserialized.identifier(), Some("catalog/path"));
        assert_eq!(deserialized.topic(), "custom.topic");
        assert_eq!(
            deserialized.precomputed_hash(),
            calculate_hash("custom.topic")
        );
    }

    #[rstest]
    fn test_data_type_deserialization_without_cache_fields_uses_constructor() {
        let payload = json!({
            "type_name": "ExampleType",
            "metadata": {"z": 9, "a": 1},
            "identifier": "catalog/path",
        });
        let expected = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"z": 9, "a": 1}))),
            Some("catalog/path".to_string()),
        );

        let deserialized: DataType = serde_json::from_value(payload).unwrap();

        assert_eq!(deserialized.type_name(), expected.type_name());
        assert_eq!(deserialized.metadata(), expected.metadata());
        assert_eq!(deserialized.identifier(), expected.identifier());
        assert_eq!(deserialized.topic(), expected.topic());
        assert_eq!(deserialized.precomputed_hash(), expected.precomputed_hash());
    }

    #[rstest]
    fn test_data_type_deserialization_preserves_topic_without_hash() {
        let payload = json!({
            "type_name": "ExampleType",
            "metadata": null,
            "topic": "custom.topic",
        });

        let deserialized: DataType = serde_json::from_value(payload).unwrap();

        assert_eq!(deserialized.topic(), "custom.topic");
        assert_eq!(
            deserialized.precomputed_hash(),
            calculate_hash("custom.topic")
        );
    }

    #[rstest]
    fn test_data_type_deserialization_ignores_hash_without_topic() {
        let expected = DataType::new("ExampleType", None, None);
        let payload = json!({
            "type_name": "ExampleType",
            "metadata": null,
            "hash": expected.precomputed_hash() ^ u64::MAX,
        });

        let deserialized: DataType = serde_json::from_value(payload).unwrap();

        assert_eq!(deserialized, expected);
        assert_eq!(deserialized.precomputed_hash(), expected.precomputed_hash());
    }

    #[rstest]
    fn test_data_type_deserialization_rejects_duplicate_map_key() {
        let payload = r#"{"type_name":"ExampleType","topic":"first","topic":"second"}"#;

        let e = serde_json::from_str::<DataType>(payload).unwrap_err();

        assert!(e.to_string().contains("duplicate field `topic`"));
    }

    #[rstest]
    #[case(
        r#"{"type_name":"ExampleType","topic":null,"topic":"second"}"#,
        "duplicate field `topic`"
    )]
    #[case(
        r#"{"type_name":"ExampleType","hash":null,"hash":7}"#,
        "duplicate field `hash`"
    )]
    #[case(
        r#"{"type_name":"ExampleType","metadata":null,"metadata":{"a":1}}"#,
        "duplicate field `metadata`"
    )]
    #[case(
        r#"{"type_name":"ExampleType","identifier":null,"identifier":"second"}"#,
        "duplicate field `identifier`"
    )]
    fn test_data_type_deserialization_rejects_duplicate_map_key_after_null(
        #[case] payload: &str,
        #[case] expected: &str,
    ) {
        let e = serde_json::from_str::<DataType>(payload).unwrap_err();

        assert!(e.to_string().contains(expected));
    }

    #[rstest]
    fn test_data_type_serde_roundtrip_preserves_fields_and_repairs_hash() {
        let payload = json!({
            "type_name": "ExampleType",
            "metadata": {"key": "value"},
            "topic": "custom.topic",
            "hash": calculate_hash("custom.topic") ^ u64::MAX,
            "identifier": "catalog/path",
        });
        let deserialized: DataType = serde_json::from_value(payload).unwrap();

        let json = serde_json::to_string(&deserialized).unwrap();
        let roundtripped: DataType = serde_json::from_str(&json).unwrap();

        assert_eq!(roundtripped.type_name(), "ExampleType");
        assert_eq!(
            roundtripped.metadata(),
            Some(&params_from_json(json!({"key": "value"})))
        );
        assert_eq!(roundtripped.identifier(), Some("catalog/path"));
        assert_eq!(roundtripped.topic(), "custom.topic");
        assert_eq!(
            roundtripped.precomputed_hash(),
            calculate_hash("custom.topic")
        );
    }

    #[rstest]
    fn test_data_type_serialized_cache_fields_remain_wire_compatible() {
        #[derive(Deserialize)]
        struct LegacyDataType {
            type_name: String,
            metadata: Option<Params>,
            topic: String,
            hash: u64,
            identifier: Option<String>,
        }

        let expected = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"key": "value"}))),
            Some("catalog/path".to_string()),
        );
        let mut payload = serde_json::to_value(&expected).unwrap();
        payload["hash"] = json!(expected.precomputed_hash() ^ u64::MAX);
        let repaired: DataType = serde_json::from_value(payload).unwrap();

        let legacy: LegacyDataType =
            serde_json::from_value(serde_json::to_value(&repaired).unwrap()).unwrap();

        assert_eq!(legacy.type_name, expected.type_name());
        assert_eq!(legacy.metadata.as_ref(), expected.metadata());
        assert_eq!(legacy.topic, expected.topic());
        assert_eq!(legacy.hash, expected.precomputed_hash());
        assert_eq!(legacy.identifier.as_deref(), expected.identifier());
    }

    #[rstest]
    fn test_data_type_topic_distinguishes_numeric_strings_from_numbers() {
        let string_value = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"value": "123"}))),
            None,
        );
        let numeric_value = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"value": 123}))),
            None,
        );

        assert_eq!(string_value.topic(), "ExampleType.value=~s~123");
        assert_eq!(numeric_value.topic(), "ExampleType.value=123");
        assert_ne!(string_value, numeric_value);
        assert_eq!(
            DataType::from_str(string_value.topic()).unwrap().metadata(),
            string_value.metadata()
        );
        assert_eq!(
            DataType::from_str(numeric_value.topic())
                .unwrap()
                .metadata(),
            numeric_value.metadata()
        );
    }

    #[rstest]
    fn test_data_type_topic_distinguishes_whole_floats_from_integers() {
        let integer_value = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"value": 1}))),
            None,
        );
        let float_value = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"value": 1.0}))),
            None,
        );

        assert_eq!(integer_value.topic(), "ExampleType.value=1");
        assert_eq!(float_value.topic(), "ExampleType.value=1.0");
        assert_ne!(integer_value, float_value);
        assert_eq!(
            DataType::from_str(float_value.topic()).unwrap().metadata(),
            float_value.metadata()
        );
    }

    #[rstest]
    fn test_data_type_topic_escapes_reserved_string_prefix() {
        let data_type = DataType::new(
            "ExampleType",
            Some(params_from_json(json!({"value": "~s~123"}))),
            None,
        );

        assert_eq!(data_type.topic(), "ExampleType.value=~s~~s~123");
        assert_eq!(
            DataType::from_str(data_type.topic()).unwrap().metadata(),
            data_type.metadata()
        );
    }

    #[rstest]
    fn test_data_type_rejects_identifier_metadata_key() {
        let error = DataType::try_new(
            "ExampleType",
            Some(params_from_json(json!({"identifier": "A"}))),
            None,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "`identifier` is reserved for DataType catalog identity"
        );
    }

    #[rstest]
    fn test_data_type_persistence_json_rejects_identifier_metadata_key() {
        let error = DataType::from_persistence_json(
            r#"{"type_name":"ExampleType","metadata":{"identifier":"A"}}"#,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "`identifier` is reserved for DataType catalog identity"
        );
    }

    #[rstest]
    fn test_data_type_creation_without_metadata() {
        let data_type = DataType::new("ExampleType", None, None);

        assert_eq!(data_type.type_name(), "ExampleType");
        assert_eq!(data_type.topic(), "ExampleType");
        assert_eq!(data_type.metadata(), None);
    }

    #[rstest]
    fn test_data_type_equality() {
        let metadata1 = Some(params_from_json(json!({"key1": "value1"})));
        let metadata2 = Some(params_from_json(json!({"key1": "value1"})));

        let data_type1 = DataType::new("ExampleType", metadata1, None);
        let data_type2 = DataType::new("ExampleType", metadata2, None);

        assert_eq!(data_type1, data_type2);
    }

    #[rstest]
    fn test_data_type_inequality() {
        let metadata1 = Some(params_from_json(json!({"key1": "value1"})));
        let metadata2 = Some(params_from_json(json!({"key2": "value2"})));

        let data_type1 = DataType::new("ExampleType", metadata1, None);
        let data_type2 = DataType::new("ExampleType", metadata2, None);

        assert_ne!(data_type1, data_type2);
    }

    #[rstest]
    fn test_data_type_ordering() {
        let metadata1 = Some(params_from_json(json!({"key1": "value1"})));
        let metadata2 = Some(params_from_json(json!({"key2": "value2"})));

        let data_type1 = DataType::new("ExampleTypeA", metadata1, None);
        let data_type2 = DataType::new("ExampleTypeB", metadata2, None);

        assert!(data_type1 < data_type2);
    }

    #[rstest]
    fn test_data_type_hash() {
        let metadata = Some(params_from_json(json!({"key1": "value1"})));

        let data_type1 = DataType::new("ExampleType", metadata.clone(), None);
        let data_type2 = DataType::new("ExampleType", metadata, None);

        let mut hasher1 = DefaultHasher::new();
        data_type1.hash(&mut hasher1);
        let hash1 = hasher1.finish();

        let mut hasher2 = DefaultHasher::new();
        data_type2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(hash1, hash2);
    }

    #[rstest]
    fn test_data_type_identifier_is_part_of_topic_identity_and_hash() {
        let data_type1 = DataType::new("ExampleType", None, Some("A".to_string()));
        let data_type2 = DataType::new("ExampleType", None, Some("B".to_string()));

        let mut hasher1 = DefaultHasher::new();
        data_type1.hash(&mut hasher1);
        let hash1 = hasher1.finish();
        let mut hasher2 = DefaultHasher::new();
        data_type2.hash(&mut hasher2);
        let hash2 = hasher2.finish();

        assert_eq!(data_type1.topic(), "ExampleType.identifier=A");
        assert_eq!(data_type2.topic(), "ExampleType.identifier=B");
        assert_ne!(data_type1, data_type2);
        assert_ne!(hash1, hash2);
    }

    #[rstest]
    fn test_data_type_display() {
        let metadata = Some(params_from_json(json!({"key1": "value1"})));
        let data_type = DataType::new("ExampleType", metadata, None);

        assert_eq!(format!("{data_type}"), "ExampleType.key1=value1");
    }

    #[rstest]
    fn test_data_type_debug() {
        let metadata = Some(params_from_json(json!({"key1": "value1"})));
        let data_type = DataType::new("ExampleType", metadata.clone(), None);

        assert_eq!(
            format!("{data_type:?}"),
            format!("DataType(type_name=ExampleType, metadata={metadata:?}, identifier=None)")
        );
    }

    #[rstest]
    fn test_parse_instrument_id_from_metadata() {
        let instrument_id_str = "MSFT.XNAS";
        let metadata = Some(params_from_json(
            json!({"instrument_id": instrument_id_str}),
        ));
        let data_type = DataType::new("InstrumentAny", metadata, None);

        assert_eq!(
            data_type.instrument_id().unwrap().unwrap(),
            InstrumentId::from_str(instrument_id_str).unwrap()
        );
    }

    #[rstest]
    fn test_parse_venue_from_metadata() {
        let venue_str = "BINANCE";
        let metadata = Some(params_from_json(json!({"venue": venue_str})));
        let data_type = DataType::new(stringify!(InstrumentAny), metadata, None);

        assert_eq!(data_type.venue().unwrap().unwrap(), Venue::new(venue_str));
    }

    #[rstest]
    fn test_parse_start_from_metadata() {
        let start_ns = 1_600_054_595_844_758_000;
        let metadata = Some(params_from_json(json!({"start": start_ns.to_string()})));
        let data_type = DataType::new(stringify!(TradeTick), metadata, None);

        assert_eq!(
            data_type.start().unwrap().unwrap(),
            UnixNanos::from(start_ns),
        );
    }

    #[rstest]
    fn test_parse_end_from_metadata() {
        let end_ns = 1_720_954_595_844_758_000;
        let metadata = Some(params_from_json(json!({"end": end_ns.to_string()})));
        let data_type = DataType::new(stringify!(TradeTick), metadata, None);

        assert_eq!(data_type.end().unwrap().unwrap(), UnixNanos::from(end_ns),);
    }

    #[rstest]
    fn test_parse_limit_from_metadata() {
        let limit = 1000;
        let metadata = Some(params_from_json(json!({"limit": limit})));
        let data_type = DataType::new(stringify!(TradeTick), metadata, None);

        assert_eq!(data_type.limit().unwrap().unwrap(), limit);
    }

    #[rstest]
    fn test_data_type_metadata_accessors_return_none_without_metadata() {
        let data_type = DataType::new(stringify!(TradeTick), None, None);

        assert_eq!(data_type.instrument_id().unwrap(), None);
        assert_eq!(data_type.venue().unwrap(), None);
        assert_eq!(data_type.start().unwrap(), None);
        assert_eq!(data_type.end().unwrap(), None);
        assert_eq!(data_type.limit().unwrap(), None);
    }

    #[rstest]
    fn test_data_type_metadata_accessors_reject_invalid_values() {
        let data_type = DataType::new(
            stringify!(TradeTick),
            Some(params_from_json(json!({
                "instrument_id": "invalid",
                "venue": "",
                "start": "invalid",
                "end": "invalid",
                "limit": "invalid",
            }))),
            None,
        );

        assert_eq!(
            data_type.instrument_id().unwrap_err().to_string(),
            "Invalid instrument_id metadata `invalid`: invalid `InstrumentId` value 'invalid': missing '.' separator between symbol and venue components"
        );
        assert_eq!(
            data_type.venue().unwrap_err().to_string(),
            "Invalid venue metadata ``: invalid string for 'value', was empty"
        );
        assert_eq!(
            data_type.start().unwrap_err().to_string(),
            "Invalid start metadata `invalid`: Invalid format: invalid"
        );
        assert_eq!(
            data_type.end().unwrap_err().to_string(),
            "Invalid end metadata `invalid`: Invalid format: invalid"
        );
        assert_eq!(
            data_type.limit().unwrap_err().to_string(),
            "Invalid limit metadata `invalid`: invalid digit found in string"
        );
    }

    #[rstest]
    fn test_data_type_persistence_json_with_identifier() {
        let data_type = DataType::new("MyCustomType", None, Some("SYMBOL.VENUE".to_string()));
        let json = data_type.to_persistence_json().unwrap();
        assert!(!json.contains("topic"));
        assert!(json.contains("\"identifier\":\"SYMBOL.VENUE\""));
        let restored = DataType::from_persistence_json(&json).unwrap();
        assert_eq!(restored.type_name(), "MyCustomType");
        assert_eq!(restored.identifier(), Some("SYMBOL.VENUE"));
        assert_eq!(restored.topic(), "MyCustomType.identifier=SYMBOL.VENUE");
    }

    #[rstest]
    fn test_data_type_from_str_parses_metadata_and_identifier() {
        let data_type = DataType::from_str(
            "MyCustomType.instrument_id=SYMBOL.VENUE.strike=1.23.identifier=SYMBOL.VENUE",
        )
        .unwrap();

        assert_eq!(data_type.type_name(), "MyCustomType");
        assert_eq!(
            data_type.metadata(),
            Some(&params_from_json(
                json!({"instrument_id": "SYMBOL.VENUE", "strike": 1.23})
            ))
        );
        assert_eq!(data_type.identifier(), Some("SYMBOL.VENUE"));
        assert_eq!(
            data_type.topic(),
            "MyCustomType.instrument_id=SYMBOL.VENUE.strike=1.23.identifier=SYMBOL.VENUE"
        );
    }

    #[rstest]
    fn test_data_type_from_persistence_json_rebuilds_canonical_topic() {
        let json = r#"{
            "type_name": "ExampleType",
            "topic": "ExampleType.z=9.a=1",
            "metadata": {"z": 9, "a": 1}
        }"#;

        let restored = DataType::from_persistence_json(json).unwrap();

        assert_eq!(restored.topic(), "ExampleType.a=1.z=9");
    }

    #[rstest]
    fn test_data_type_persistence_result_hashes_like_equal_deserialized_value() {
        let persistence_json = r#"{
            "type_name": "ExampleType",
            "topic": "ignored.legacy.topic",
            "metadata": {"z": 9, "a": 1},
            "identifier": "catalog/path"
        }"#;
        let persisted = DataType::from_persistence_json(persistence_json).unwrap();
        let payload = json!({
            "type_name": persisted.type_name(),
            "metadata": persisted.metadata(),
            "topic": persisted.topic(),
            "hash": persisted.precomputed_hash() ^ u64::MAX,
            "identifier": persisted.identifier(),
        });
        let deserialized: DataType = serde_json::from_value(payload).unwrap();
        let hash = |data_type: &DataType| {
            let mut hasher = DefaultHasher::new();
            data_type.hash(&mut hasher);
            hasher.finish()
        };

        assert_eq!(
            persisted.topic(),
            "ExampleType.a=1.z=9.identifier=catalog/path"
        );
        assert_eq!(persisted.identifier(), Some("catalog/path"));
        assert_eq!(deserialized, persisted);
        assert_eq!(hash(&deserialized), hash(&persisted));
    }

    #[rstest]
    fn test_data_type_identifier_getter() {
        let data_type = DataType::new("T", None, Some("id".to_string()));
        assert_eq!(data_type.identifier(), Some("id"));
        let data_type_no_id = DataType::new("T", None, None);
        assert_eq!(data_type_no_id.identifier(), None);
    }
}
