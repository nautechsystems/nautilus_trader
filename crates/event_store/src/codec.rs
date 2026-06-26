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

//! Hardened positional serde codec for event-store records.

use std::{char, fmt::Display, str};

use serde::{
    Serialize,
    de::{
        self, DeserializeOwned, DeserializeSeed, EnumAccess, IntoDeserializer, MapAccess,
        SeqAccess, VariantAccess, Visitor,
    },
    ser::{
        self, SerializeMap, SerializeSeq, SerializeStruct, SerializeStructVariant, SerializeTuple,
        SerializeTupleStruct, SerializeTupleVariant,
    },
};
use thiserror::Error;

const MAGIC: [u8; 4] = *b"NESC";
const VERSION: u8 = 1;
const HEADER_LEN: usize = MAGIC.len() + 1;

/// Serializes `value` into a freshly allocated, framed codec buffer.
///
/// The output is `MAGIC ++ VERSION ++ body`. Deterministic: equal values produce
/// byte-identical output.
///
/// # Errors
///
/// Returns [`CodecError`] if `value`'s [`Serialize`] impl drives the format outside the
/// supported positional model, such as an unbounded sequence whose length is not known up front.
pub fn encode_to_vec<T: Serialize>(value: &T) -> Result<Vec<u8>, CodecError> {
    let mut encoder = Encoder { out: Vec::new() };
    encoder.out.extend_from_slice(&MAGIC);
    encoder.out.push(VERSION);
    value.serialize(&mut encoder)?;
    Ok(encoder.out)
}

/// Decodes a `T` from a complete framed codec buffer.
///
/// Validates the frame header, decodes the body positionally, then requires the input to be fully
/// consumed.
///
/// # Errors
///
/// Returns [`CodecError`] on a bad or short header, malformed body, a self-describing deserialize
/// request, or unconsumed trailing bytes.
pub fn decode_from_slice<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, CodecError> {
    let mut decoder = Decoder {
        input: bytes,
        pos: 0,
    };
    decoder.read_header()?;
    let value = T::deserialize(&mut decoder)?;
    if decoder.pos != decoder.input.len() {
        return Err(CodecError::TrailingBytes(decoder.input.len() - decoder.pos));
    }
    Ok(value)
}

/// Errors returned by the event-store positional codec.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum CodecError {
    /// The input ended before the requested bytes could be read.
    #[error("unexpected end of codec input: needed {needed} more byte(s)")]
    UnexpectedEof {
        /// Number of bytes the decoder attempted to read.
        needed: usize,
    },
    /// The frame did not start with the event-store codec magic.
    #[error("bad codec magic: input is not a nautilus event-store codec frame")]
    BadMagic,
    /// The frame version is not supported by this decoder.
    #[error("unsupported codec version: {0}")]
    UnsupportedVersion(u8),
    /// An encoded length would exceed the remaining input.
    #[error("encoded length {claimed} exceeds remaining input {remaining}")]
    LengthOverflow {
        /// Claimed encoded length or collection count, as the raw on-wire `u64`.
        ///
        /// Kept as `u64` (not `usize`) so the reported value is exact on every
        /// target, including a length that overflows `usize` on a sub-64-bit
        /// build — the case that would otherwise be reported with a lossy
        /// sentinel.
        claimed: u64,
        /// Bytes remaining in the input when the length was checked.
        remaining: usize,
    },
    /// A string was not valid UTF-8.
    #[error("invalid utf-8 in encoded string")]
    InvalidUtf8,
    /// A bool discriminant was not `0x00` or `0x01`.
    #[error("invalid bool discriminant: {0:#04x}")]
    InvalidBool(u8),
    /// An option discriminant was not `0x00` or `0x01`.
    #[error("invalid option discriminant: {0:#04x}")]
    InvalidOption(u8),
    /// A decoded char value was not a valid Unicode scalar value.
    #[error("invalid char scalar value: {0:#010x}")]
    InvalidChar(u32),
    /// An enum variant index was outside the type's variant set.
    #[error("unknown enum variant index: {0}")]
    UnknownVariant(u32),
    /// Serde did not provide a sequence or map length.
    #[error("sequence length was not provided by the serializer")]
    MissingLen,
    /// The input had bytes remaining after a complete value was decoded.
    #[error("{0} unconsumed trailing byte(s) after decode")]
    TrailingBytes(usize),
    /// The caller attempted self-describing deserialization.
    #[error("self-describing deserialization is not supported by this format")]
    SelfDescribing,
    /// Serde-generated error message.
    #[error("{0}")]
    Message(String),
}

impl ser::Error for CodecError {
    fn custom<T: Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}

impl de::Error for CodecError {
    fn custom<T: Display>(msg: T) -> Self {
        Self::Message(msg.to_string())
    }
}

#[derive(Debug)]
struct Encoder {
    out: Vec<u8>,
}

impl Encoder {
    fn write_len(&mut self, len: usize) {
        self.out.extend_from_slice(&(len as u64).to_le_bytes());
    }

    fn write_variant(&mut self, variant_index: u32) {
        self.out.extend_from_slice(&variant_index.to_le_bytes());
    }
}

impl ser::Serializer for &mut Encoder {
    type Ok = ();
    type Error = CodecError;
    type SerializeSeq = Self;
    type SerializeTuple = Self;
    type SerializeTupleStruct = Self;
    type SerializeTupleVariant = Self;
    type SerializeMap = Self;
    type SerializeStruct = Self;
    type SerializeStructVariant = Self;

    fn serialize_bool(self, v: bool) -> Result<Self::Ok, Self::Error> {
        self.out.push(u8::from(v));
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<Self::Ok, Self::Error> {
        self.out.push(v);
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&v.to_le_bytes());
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<Self::Ok, Self::Error> {
        self.out.extend_from_slice(&(v as u32).to_le_bytes());
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<Self::Ok, Self::Error> {
        self.write_len(v.len());
        self.out.extend_from_slice(v.as_bytes());
        Ok(())
    }

    fn serialize_bytes(self, v: &[u8]) -> Result<Self::Ok, Self::Error> {
        self.write_len(v.len());
        self.out.extend_from_slice(v);
        Ok(())
    }

    fn serialize_none(self) -> Result<Self::Ok, Self::Error> {
        self.out.push(0);
        Ok(())
    }

    fn serialize_some<T>(self, value: &T) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.out.push(1);
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
    ) -> Result<Self::Ok, Self::Error> {
        self.write_variant(variant_index);
        Ok(())
    }

    fn serialize_newtype_struct<T>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T>(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        value: &T,
    ) -> Result<Self::Ok, Self::Error>
    where
        T: ?Sized + Serialize,
    {
        self.write_variant(variant_index);
        value.serialize(self)
    }

    fn serialize_seq(self, len: Option<usize>) -> Result<Self::SerializeSeq, Self::Error> {
        let len = len.ok_or(CodecError::MissingLen)?;
        self.write_len(len);
        Ok(self)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant, Self::Error> {
        self.write_variant(variant_index);
        Ok(self)
    }

    fn serialize_map(self, len: Option<usize>) -> Result<Self::SerializeMap, Self::Error> {
        let len = len.ok_or(CodecError::MissingLen)?;
        self.write_len(len);
        Ok(self)
    }

    fn serialize_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStruct, Self::Error> {
        Ok(self)
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant, Self::Error> {
        self.write_variant(variant_index);
        Ok(self)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

impl SerializeSeq for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTuple for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_element<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTupleStruct for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeTupleVariant for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_field<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeMap for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_key<T>(&mut self, key: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        key.serialize(&mut **self)
    }

    fn serialize_value<T>(&mut self, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStruct for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

impl SerializeStructVariant for &mut Encoder {
    type Ok = ();
    type Error = CodecError;

    fn serialize_field<T>(&mut self, _key: &'static str, value: &T) -> Result<(), Self::Error>
    where
        T: ?Sized + Serialize,
    {
        value.serialize(&mut **self)
    }

    fn end(self) -> Result<Self::Ok, Self::Error> {
        Ok(())
    }
}

#[derive(Debug)]
struct Decoder<'de> {
    input: &'de [u8],
    pos: usize,
}

impl<'de> Decoder<'de> {
    fn read_header(&mut self) -> Result<(), CodecError> {
        if self.input.len() < HEADER_LEN {
            return Err(CodecError::UnexpectedEof { needed: HEADER_LEN });
        }

        let magic = self.take(MAGIC.len())?;
        if magic != MAGIC {
            return Err(CodecError::BadMagic);
        }

        let version = self.take(1)?[0];
        if version != VERSION {
            return Err(CodecError::UnsupportedVersion(version));
        }

        Ok(())
    }

    fn remaining(&self) -> usize {
        self.input.len() - self.pos
    }

    fn take(&mut self, n: usize) -> Result<&'de [u8], CodecError> {
        let end = self
            .pos
            .checked_add(n)
            .ok_or(CodecError::UnexpectedEof { needed: n })?;
        let slice = self
            .input
            .get(self.pos..end)
            .ok_or(CodecError::UnexpectedEof { needed: n })?;
        self.pos = end;
        Ok(slice)
    }

    fn read_len(&mut self) -> Result<usize, CodecError> {
        let raw = self.read_u64()?;
        let remaining = self.remaining();
        // A length is valid only if it both fits `usize` and is within the
        // remaining input. Any failure reports the exact `u64` wire value via
        // `claimed`, so the overflow case needs no lossy `usize` sentinel.
        match usize::try_from(raw) {
            Ok(claimed) if claimed <= remaining => Ok(claimed),
            _ => Err(CodecError::LengthOverflow {
                claimed: raw,
                remaining,
            }),
        }
    }

    fn read_u16(&mut self) -> Result<u16, CodecError> {
        let bytes: [u8; 2] = self
            .take(2)?
            .try_into()
            .expect("take returned the exact requested width");
        Ok(u16::from_le_bytes(bytes))
    }

    fn read_u32(&mut self) -> Result<u32, CodecError> {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .expect("take returned the exact requested width");
        Ok(u32::from_le_bytes(bytes))
    }

    fn read_u64(&mut self) -> Result<u64, CodecError> {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .expect("take returned the exact requested width");
        Ok(u64::from_le_bytes(bytes))
    }
}

macro_rules! deserialize_integer {
    ($method:ident, $visit:ident, $ty:ty, $width:literal) => {
        fn $method<V>(self, visitor: V) -> Result<V::Value, Self::Error>
        where
            V: Visitor<'de>,
        {
            let bytes: [u8; $width] = self
                .take($width)?
                .try_into()
                .expect("take returned the exact requested width");
            visitor.$visit(<$ty>::from_le_bytes(bytes))
        }
    };
}

impl<'de> de::Deserializer<'de> for &mut Decoder<'de> {
    type Error = CodecError;

    fn deserialize_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(CodecError::SelfDescribing)
    }

    fn deserialize_bool<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.take(1)?[0] {
            0 => visitor.visit_bool(false),
            1 => visitor.visit_bool(true),
            other => Err(CodecError::InvalidBool(other)),
        }
    }

    fn deserialize_i8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let bytes: [u8; 1] = self
            .take(1)?
            .try_into()
            .expect("take returned the exact requested width");
        visitor.visit_i8(i8::from_le_bytes(bytes))
    }

    deserialize_integer!(deserialize_i16, visit_i16, i16, 2);
    deserialize_integer!(deserialize_i32, visit_i32, i32, 4);
    deserialize_integer!(deserialize_i64, visit_i64, i64, 8);

    fn deserialize_u8<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u8(self.take(1)?[0])
    }

    fn deserialize_u16<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u16(self.read_u16()?)
    }

    fn deserialize_u32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u32(self.read_u32()?)
    }

    fn deserialize_u64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_u64(self.read_u64()?)
    }

    fn deserialize_f32<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let bytes: [u8; 4] = self
            .take(4)?
            .try_into()
            .expect("take returned the exact requested width");
        visitor.visit_f32(f32::from_le_bytes(bytes))
    }

    fn deserialize_f64<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let bytes: [u8; 8] = self
            .take(8)?
            .try_into()
            .expect("take returned the exact requested width");
        visitor.visit_f64(f64::from_le_bytes(bytes))
    }

    fn deserialize_char<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let raw = self.read_u32()?;
        let value = char::from_u32(raw).ok_or(CodecError::InvalidChar(raw))?;
        visitor.visit_char(value)
    }

    fn deserialize_str<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let len = self.read_len()?;
        let bytes = self.take(len)?;
        let value = str::from_utf8(bytes).map_err(|_| CodecError::InvalidUtf8)?;
        visitor.visit_borrowed_str(value)
    }

    fn deserialize_string<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_str(visitor)
    }

    fn deserialize_bytes<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let len = self.read_len()?;
        let bytes = self.take(len)?;
        visitor.visit_borrowed_bytes(bytes)
    }

    fn deserialize_byte_buf<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_bytes(visitor)
    }

    fn deserialize_option<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        match self.take(1)?[0] {
            0 => visitor.visit_none(),
            1 => visitor.visit_some(self),
            other => Err(CodecError::InvalidOption(other)),
        }
    }

    fn deserialize_unit<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_unit_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_unit()
    }

    fn deserialize_newtype_struct<V>(
        self,
        _name: &'static str,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_newtype_struct(self)
    }

    fn deserialize_seq<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let remaining = self.read_len()?;
        visitor.visit_seq(SeqReader {
            dec: self,
            remaining,
        })
    }

    fn deserialize_tuple<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(SeqReader {
            dec: self,
            remaining: len,
        })
    }

    fn deserialize_tuple_struct<V>(
        self,
        _name: &'static str,
        len: usize,
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_tuple(len, visitor)
    }

    fn deserialize_map<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let remaining = self.read_len()?;
        visitor.visit_map(MapReader {
            dec: self,
            remaining,
        })
    }

    fn deserialize_struct<V>(
        self,
        _name: &'static str,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(SeqReader {
            dec: self,
            remaining: fields.len(),
        })
    }

    fn deserialize_enum<V>(
        self,
        _name: &'static str,
        variants: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        let index = self.read_u32()?;
        if index as usize >= variants.len() {
            return Err(CodecError::UnknownVariant(index));
        }
        visitor.visit_enum(EnumReader { dec: self, index })
    }

    fn deserialize_identifier<V>(self, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        self.deserialize_u32(visitor)
    }

    fn deserialize_ignored_any<V>(self, _visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        Err(CodecError::SelfDescribing)
    }

    fn is_human_readable(&self) -> bool {
        false
    }
}

#[derive(Debug)]
struct SeqReader<'a, 'de> {
    dec: &'a mut Decoder<'de>,
    remaining: usize,
}

impl<'de> SeqAccess<'de> for SeqReader<'_, 'de> {
    type Error = CodecError;

    fn next_element_seed<T>(&mut self, seed: T) -> Result<Option<T::Value>, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        seed.deserialize(&mut *self.dec).map(Some)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

#[derive(Debug)]
struct MapReader<'a, 'de> {
    dec: &'a mut Decoder<'de>,
    remaining: usize,
}

impl<'de> MapAccess<'de> for MapReader<'_, 'de> {
    type Error = CodecError;

    fn next_key_seed<K>(&mut self, seed: K) -> Result<Option<K::Value>, Self::Error>
    where
        K: DeserializeSeed<'de>,
    {
        if self.remaining == 0 {
            return Ok(None);
        }

        self.remaining -= 1;
        seed.deserialize(&mut *self.dec).map(Some)
    }

    fn next_value_seed<V>(&mut self, seed: V) -> Result<V::Value, Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        seed.deserialize(&mut *self.dec)
    }

    fn size_hint(&self) -> Option<usize> {
        Some(self.remaining)
    }
}

#[derive(Debug)]
struct EnumReader<'a, 'de> {
    dec: &'a mut Decoder<'de>,
    index: u32,
}

impl<'de> EnumAccess<'de> for EnumReader<'_, 'de> {
    type Error = CodecError;
    type Variant = Self;

    fn variant_seed<V>(self, seed: V) -> Result<(V::Value, Self::Variant), Self::Error>
    where
        V: DeserializeSeed<'de>,
    {
        let value = seed.deserialize(self.index.into_deserializer())?;
        Ok((value, self))
    }
}

impl<'de> VariantAccess<'de> for EnumReader<'_, 'de> {
    type Error = CodecError;

    fn unit_variant(self) -> Result<(), Self::Error> {
        Ok(())
    }

    fn newtype_variant_seed<T>(self, seed: T) -> Result<T::Value, Self::Error>
    where
        T: DeserializeSeed<'de>,
    {
        seed.deserialize(self.dec)
    }

    fn tuple_variant<V>(self, len: usize, visitor: V) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(SeqReader {
            dec: self.dec,
            remaining: len,
        })
    }

    fn struct_variant<V>(
        self,
        fields: &'static [&'static str],
        visitor: V,
    ) -> Result<V::Value, Self::Error>
    where
        V: Visitor<'de>,
    {
        visitor.visit_seq(SeqReader {
            dec: self.dec,
            remaining: fields.len(),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::array;

    use bytes::Bytes;
    use indexmap::IndexMap;
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_system::RegisteredComponents;
    use proptest::{prelude::*, test_runner::Config as ProptestConfig};
    use rstest::rstest;
    use serde::Deserialize;
    use ustr::Ustr;

    use super::*;
    use crate::{
        EventStoreEntry, Headers, RunManifest, RunStatus, SnapshotAnchor,
        hash::{EntryHash, compute_entry_hash},
        markers::{
            DataClass, DataCursorSnapshot, HiFiMarker, MarkerGap, MarkerGapReason, StreamCursor,
            StreamDictEntry,
        },
    };

    #[derive(Debug, Serialize, Deserialize)]
    struct ScalarProbe {
        b: bool,
        i8s: [i8; 3],
        i16s: [i16; 3],
        i32s: [i32; 3],
        i64s: [i64; 3],
        u16: u16,
        f32s: [f32; 3],
        f64s: [f64; 3],
        ch: char,
    }

    impl PartialEq for ScalarProbe {
        fn eq(&self, other: &Self) -> bool {
            self.b == other.b
                && self.i8s == other.i8s
                && self.i16s == other.i16s
                && self.i32s == other.i32s
                && self.i64s == other.i64s
                && self.u16 == other.u16
                && self
                    .f32s
                    .iter()
                    .zip(other.f32s)
                    .all(|(left, right)| left.to_bits() == right.to_bits())
                && self
                    .f64s
                    .iter()
                    .zip(other.f64s)
                    .all(|(left, right)| left.to_bits() == right.to_bits())
                && self.ch == other.ch
        }
    }

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct BoolProbe {
        b: bool,
    }

    fn roundtrip<T>(value: &T) -> T
    where
        T: Serialize + DeserializeOwned,
    {
        decode_from_slice(&encode_to_vec(value).expect("encode")).expect("decode")
    }

    fn headers_populated() -> Headers {
        Headers {
            correlation_id: Some(UUID4::from_bytes([1; 16])),
            causation_id: Some(UUID4::from_bytes([2; 16])),
        }
    }

    fn entry(headers: Headers) -> EventStoreEntry {
        let topic = "exec.command".into();
        let payload_type = Ustr::from("SubmitOrder");
        let payload = Bytes::from_static(b"\x01\x02\x03\x04");
        let seq = 42;
        let ts_init = UnixNanos::from(1_700_000_000_000_000_000);
        let ts_publish = UnixNanos::from(1_700_000_000_000_000_001);
        let entry_hash = compute_entry_hash(
            seq,
            ts_init,
            ts_publish,
            "exec.command",
            payload_type.as_str(),
            &payload,
            &headers,
        );

        EventStoreEntry::new(
            entry_hash,
            seq,
            headers,
            topic,
            payload_type,
            payload,
            ts_init,
            ts_publish,
        )
    }

    fn registered_components() -> RegisteredComponents {
        let mut components = RegisteredComponents::default();
        components
            .actors
            .insert("actor-1".to_string(), "hash-a".to_string());
        components
            .strategies
            .insert("strategy-1".to_string(), "hash-s".to_string());
        components
            .algorithms
            .insert("algo-1".to_string(), "hash-g".to_string());
        components.subscriptions.push("data.quotes".to_string());
        components.endpoints.push("exec.command".to_string());
        components
    }

    fn running_manifest() -> RunManifest {
        RunManifest {
            run_id: "1700000000-abcd1234".to_string(),
            parent_run_id: None,
            instance_id: "trader-001".to_string(),
            binary_hash: "deadbeef".to_string(),
            schema_version: 1,
            crate_versions: "feedface".to_string(),
            feature_flags: Vec::new(),
            adapter_versions: IndexMap::new(),
            config_hash: "cafebabe".to_string(),
            registered_components: RegisteredComponents::default(),
            seed: None,
            start_ts_init: UnixNanos::from(10),
            end_ts_init: None,
            high_watermark: 0,
            status: RunStatus::Running,
        }
    }

    fn sealed_manifest() -> RunManifest {
        let mut adapter_versions = IndexMap::new();
        adapter_versions.insert("binance".to_string(), "1.2.3".to_string());
        adapter_versions.insert("okx".to_string(), "2.3.4".to_string());

        RunManifest {
            run_id: "1700000010-cafe1234".to_string(),
            parent_run_id: Some("1700000000-abcd1234".to_string()),
            instance_id: "trader-001".to_string(),
            binary_hash: "deadbeef".to_string(),
            schema_version: 2,
            crate_versions: "feedface".to_string(),
            feature_flags: vec!["live".to_string(), "persistence".to_string()],
            adapter_versions,
            config_hash: "cafebabe".to_string(),
            registered_components: registered_components(),
            seed: Some(7),
            start_ts_init: UnixNanos::from(10),
            end_ts_init: Some(UnixNanos::from(20)),
            high_watermark: 99,
            status: RunStatus::Ended,
        }
    }

    fn snapshot_with_cursors() -> DataCursorSnapshot {
        DataCursorSnapshot {
            marker_seq: 7,
            event_seq_before: 42,
            ts_init: UnixNanos::from(100),
            advanced: vec![
                StreamCursor {
                    slot: 1,
                    ts_init_hi: UnixNanos::from(101),
                    count: 10,
                },
                StreamCursor {
                    slot: 2,
                    ts_init_hi: UnixNanos::from(102),
                    count: 11,
                },
            ],
        }
    }

    fn hifi_marker() -> HiFiMarker {
        HiFiMarker {
            marker_seq: 1,
            event_seq_before: 42,
            slot: 3,
            ts_event: UnixNanos::from(1000),
            ts_init: UnixNanos::from(1001),
            same_ts_ordinal: 2,
            record_fingerprint: array::from_fn(|idx| {
                u8::try_from(idx).expect("fingerprint index is in 0..32")
            }),
        }
    }

    #[rstest]
    #[case::empty(Headers::empty())]
    #[case::populated(headers_populated())]
    fn roundtrip_event_store_entry(#[case] headers: Headers) {
        let decoded = roundtrip(&entry(headers));

        assert_eq!(decoded.recompute_hash(), decoded.entry_hash);
    }

    #[rstest]
    #[case::running(running_manifest())]
    #[case::sealed(sealed_manifest())]
    fn roundtrip_run_manifest(#[case] manifest: RunManifest) {
        assert_eq!(roundtrip(&manifest), manifest);
    }

    #[rstest]
    fn roundtrip_registered_components() {
        let components = registered_components();

        assert_eq!(roundtrip(&components), components);
    }

    #[rstest]
    fn roundtrip_snapshot_anchor() {
        let anchor = SnapshotAnchor::new(10, "cache://run/10", "blake3:abcd");

        assert_eq!(roundtrip(&anchor), anchor);
    }

    #[rstest]
    #[case::advanced(snapshot_with_cursors())]
    #[case::empty(DataCursorSnapshot {
        marker_seq: 8,
        event_seq_before: 43,
        ts_init: UnixNanos::from(101),
        advanced: Vec::new(),
    })]
    fn roundtrip_data_cursor_snapshot(#[case] snapshot: DataCursorSnapshot) {
        assert_eq!(roundtrip(&snapshot), snapshot);
    }

    #[rstest]
    fn roundtrip_hifi_marker() {
        let marker = hifi_marker();

        assert_eq!(roundtrip(&marker), marker);
    }

    #[rstest]
    #[case(MarkerGapReason::Overflow)]
    #[case(MarkerGapReason::WriterClosed)]
    fn roundtrip_marker_gap(#[case] reason: MarkerGapReason) {
        let gap = MarkerGap {
            from_marker_seq: 1,
            to_marker_seq: 2,
            reason,
        };

        assert_eq!(roundtrip(&gap), gap);
    }

    #[rstest]
    fn roundtrip_stream_dict_entry() {
        let entry = StreamDictEntry {
            slot: 3,
            data_cls: DataClass::Quote,
            identifier: "ETHUSDT.BINANCE".to_string(),
        };

        assert_eq!(roundtrip(&entry), entry);
    }

    #[rstest]
    #[case(RunStatus::Running)]
    #[case(RunStatus::Ended)]
    #[case(RunStatus::CrashedRecovered)]
    #[case(RunStatus::Quarantined)]
    fn roundtrip_run_status_all_variants(#[case] status: RunStatus) {
        assert_eq!(roundtrip(&status), status);
    }

    #[rstest]
    #[case(DataClass::BookDeltas)]
    #[case(DataClass::BookDepth10)]
    #[case(DataClass::Quote)]
    #[case(DataClass::Trade)]
    #[case(DataClass::Bar)]
    fn roundtrip_data_class_all_variants(#[case] data_class: DataClass) {
        assert_eq!(roundtrip(&data_class), data_class);
    }

    #[rstest]
    #[case(MarkerGapReason::Overflow)]
    #[case(MarkerGapReason::WriterClosed)]
    fn roundtrip_marker_gap_reason_all_variants(#[case] reason: MarkerGapReason) {
        assert_eq!(roundtrip(&reason), reason);
    }

    #[rstest]
    #[case::false_value(false)]
    #[case::true_value(true)]
    fn roundtrip_scalars(#[case] value: bool) {
        let probe = ScalarProbe {
            b: value,
            i8s: [i8::MIN, -1, i8::MAX],
            i16s: [i16::MIN, -2, i16::MAX],
            i32s: [i32::MIN, -3, i32::MAX],
            i64s: [i64::MIN, -4, i64::MAX],
            u16: u16::MAX,
            f32s: [0.0, 1.25, f32::NAN],
            f64s: [0.0, f64::INFINITY, f64::NAN],
            ch: '∞',
        };

        assert_eq!(roundtrip(&probe), probe);
    }

    #[rstest]
    fn encode_is_deterministic() {
        let entry = entry(headers_populated());
        let manifest = sealed_manifest();

        assert_eq!(
            encode_to_vec(&entry).unwrap(),
            encode_to_vec(&entry).unwrap()
        );
        assert_eq!(
            encode_to_vec(&manifest).unwrap(),
            encode_to_vec(&manifest).unwrap()
        );
    }

    #[rstest]
    fn header_present_and_correct() {
        let bytes = encode_to_vec(&RunStatus::Running).unwrap();

        assert_eq!(&bytes[..4], b"NESC");
        assert_eq!(bytes[4], 1);
    }

    #[rstest]
    fn header_bad_magic_rejected() {
        let mut bytes = encode_to_vec(&entry(Headers::empty())).unwrap();
        bytes[0] ^= 0xFF;

        assert!(matches!(
            decode_from_slice::<EventStoreEntry>(&bytes),
            Err(CodecError::BadMagic)
        ));
    }

    #[rstest]
    fn header_unsupported_version_rejected() {
        let mut bytes = encode_to_vec(&entry(Headers::empty())).unwrap();
        bytes[4] = 2;

        assert!(matches!(
            decode_from_slice::<EventStoreEntry>(&bytes),
            Err(CodecError::UnsupportedVersion(2))
        ));
    }

    #[rstest]
    fn header_truncated_rejected() {
        let bytes = encode_to_vec(&entry(Headers::empty())).unwrap();

        assert!(matches!(
            decode_from_slice::<EventStoreEntry>(&bytes[..3]),
            Err(CodecError::UnexpectedEof { .. })
        ));
    }

    #[rstest]
    fn decodes_a_bincode_blob_as_bad_magic() {
        let cfg = bincode::config::standard();
        let bytes = bincode::serde::encode_to_vec("old-format".to_string(), cfg).unwrap();

        assert!(matches!(
            decode_from_slice::<String>(&bytes),
            Err(CodecError::BadMagic)
        ));
    }

    #[rstest]
    fn truncated_body_rejected() {
        let bytes = encode_to_vec(&entry(Headers::empty())).unwrap();

        assert!(decode_from_slice::<EventStoreEntry>(&bytes[..bytes.len() - 1]).is_err());
    }

    #[rstest]
    fn forged_length_prefix_rejected() {
        let mut bytes = encode_to_vec(&"ok".to_string()).unwrap();
        bytes[HEADER_LEN..HEADER_LEN + 8].copy_from_slice(&u64::MAX.to_le_bytes());

        assert!(matches!(
            decode_from_slice::<String>(&bytes),
            Err(CodecError::LengthOverflow { .. })
        ));
    }

    #[rstest]
    fn length_overflow_rejected() {
        let mut bytes = encode_to_vec(&vec![1_u8, 2, 3]).unwrap();
        bytes[HEADER_LEN..HEADER_LEN + 8].copy_from_slice(&99_u64.to_le_bytes());

        assert!(matches!(
            decode_from_slice::<Vec<u8>>(&bytes),
            Err(CodecError::LengthOverflow { claimed: 99, .. })
        ));
    }

    #[rstest]
    fn bad_bool_discriminant_rejected() {
        let mut bytes = encode_to_vec(&BoolProbe { b: false }).unwrap();
        bytes[HEADER_LEN] = 2;

        assert!(matches!(
            decode_from_slice::<BoolProbe>(&bytes),
            Err(CodecError::InvalidBool(2))
        ));
    }

    #[rstest]
    fn bad_option_discriminant_rejected() {
        let mut bytes = encode_to_vec(&Headers::empty()).unwrap();
        bytes[HEADER_LEN] = 2;

        assert!(matches!(
            decode_from_slice::<Headers>(&bytes),
            Err(CodecError::InvalidOption(2))
        ));
    }

    #[rstest]
    fn invalid_utf8_rejected() {
        let mut bytes = encode_to_vec(&"ok".to_string()).unwrap();
        bytes[HEADER_LEN + 8] = 0xFF;

        assert!(matches!(
            decode_from_slice::<String>(&bytes),
            Err(CodecError::InvalidUtf8)
        ));
    }

    #[rstest]
    fn unknown_enum_variant_rejected() {
        let mut bytes = encode_to_vec(&RunStatus::Running).unwrap();
        bytes[HEADER_LEN..HEADER_LEN + 4].copy_from_slice(&99_u32.to_le_bytes());

        assert!(matches!(
            decode_from_slice::<RunStatus>(&bytes),
            Err(CodecError::UnknownVariant(99))
        ));
    }

    #[rstest]
    fn trailing_bytes_rejected() {
        let mut bytes = encode_to_vec(&RunStatus::Running).unwrap();
        bytes.push(0xFF);

        assert!(matches!(
            decode_from_slice::<RunStatus>(&bytes),
            Err(CodecError::TrailingBytes(1))
        ));
    }

    #[rstest]
    fn rejects_self_describing() {
        #[allow(dead_code)]
        #[derive(Debug, Deserialize)]
        #[serde(untagged)]
        enum Probe {
            A(u64),
            B(String),
        }

        let bytes = encode_to_vec(&1_u64).unwrap();
        let err = decode_from_slice::<Probe>(&bytes).expect_err("untagged enum must reject any");
        assert!(matches!(err, CodecError::SelfDescribing));
    }

    #[rstest]
    fn serialize_without_known_len_is_rejected() {
        let mut encoder = Encoder { out: Vec::new() };
        assert!(matches!(
            ser::Serializer::serialize_seq(&mut encoder, None),
            Err(CodecError::MissingLen)
        ));

        let mut encoder = Encoder { out: Vec::new() };
        assert!(matches!(
            ser::Serializer::serialize_map(&mut encoder, None),
            Err(CodecError::MissingLen)
        ));
    }

    struct U32Visitor;

    impl Visitor<'_> for U32Visitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            formatter.write_str("a little-endian u32 identifier")
        }

        fn visit_u32<E>(self, value: u32) -> Result<Self::Value, E>
        where
            E: de::Error,
        {
            Ok(value)
        }
    }

    #[rstest]
    fn deserialize_identifier_reads_u32() {
        // `deserialize_identifier` is implemented for totality only (struct
        // fields decode positionally and variant indices flow through serde's
        // `U32Deserializer`, so the derive never reaches it). Drive it directly
        // to prove the laid brick: it forwards to the little-endian `u32` reader.
        let body = 7_u32.to_le_bytes();
        let mut decoder = Decoder {
            input: &body,
            pos: 0,
        };

        let value = de::Deserializer::deserialize_identifier(&mut decoder, U32Visitor)
            .expect("identifier decodes as a u32");

        assert_eq!(value, 7);
        assert_eq!(decoder.pos, body.len());
    }

    #[rstest]
    fn deserialize_ignored_any_rejected() {
        // `deserialize_ignored_any` is implemented for totality only and is hard
        // to reach via the positional model, but it carries the same
        // self-describing-rejection load as `deserialize_any` (the `wire.rs`
        // invariant). `IgnoredAny::deserialize` drives it directly; assert it
        // rejects rather than silently skipping.
        let mut decoder = Decoder { input: &[], pos: 0 };

        let err = de::IgnoredAny::deserialize(&mut decoder)
            .expect_err("ignored_any must reject as self-describing");

        assert!(matches!(err, CodecError::SelfDescribing));
    }

    #[rstest]
    fn entry_hash_newtype_roundtrips() {
        let hash = EntryHash(array::from_fn(|idx| {
            u8::try_from(idx).expect("hash index is in 0..32")
        }));

        assert_eq!(roundtrip(&hash), hash);
    }

    proptest! {
        #![proptest_config(ProptestConfig { cases: 64, ..ProptestConfig::default() })]

        #[rstest]
        fn prop_roundtrip_data_cursor_snapshot(
            marker_seq in any::<u64>(),
            event_seq_before in any::<u64>(),
            ts_init in any::<u64>(),
            cursors in proptest::collection::vec((any::<u32>(), any::<u64>(), any::<u64>()), 0..8),
        ) {
            let snap = DataCursorSnapshot {
                marker_seq,
                event_seq_before,
                ts_init: UnixNanos::from(ts_init),
                advanced: cursors
                    .into_iter()
                    .map(|(slot, hi, count)| StreamCursor {
                        slot,
                        ts_init_hi: UnixNanos::from(hi),
                        count,
                    })
                    .collect(),
            };

            let bytes = encode_to_vec(&snap).expect("encode");
            let decoded: DataCursorSnapshot = decode_from_slice(&bytes).expect("decode");
            prop_assert_eq!(snap, decoded);
        }

        #[rstest]
        fn prop_roundtrip_hifi_marker(
            marker_seq in any::<u64>(),
            event_seq_before in any::<u64>(),
            slot in any::<u32>(),
            ts_event in any::<u64>(),
            ts_init in any::<u64>(),
            same_ts_ordinal in any::<u32>(),
            fingerprint in proptest::array::uniform32(any::<u8>()),
        ) {
            let marker = HiFiMarker {
                marker_seq,
                event_seq_before,
                slot,
                ts_event: UnixNanos::from(ts_event),
                ts_init: UnixNanos::from(ts_init),
                same_ts_ordinal,
                record_fingerprint: fingerprint,
            };

            let bytes = encode_to_vec(&marker).expect("encode");
            let decoded: HiFiMarker = decode_from_slice(&bytes).expect("decode");
            prop_assert_eq!(marker, decoded);
        }
    }
}
