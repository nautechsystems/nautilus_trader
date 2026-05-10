pub use decoder::AccountCommissionResponseDecoder;
pub use encoder::AccountCommissionResponseEncoder;

use super::*;
pub use super::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SBE_SEMANTIC_VERSION};

pub const SBE_BLOCK_LENGTH: u16 = 108;
pub const SBE_TEMPLATE_ID: u16 = 405;

pub mod encoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Debug, Default)]
    pub struct AccountCommissionResponseEncoder<'a> {
        buf: WriteBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
    }

    impl<'a> Writer<'a> for AccountCommissionResponseEncoder<'a> {
        #[inline]
        fn get_buf_mut(&mut self) -> &mut WriteBuf<'a> {
            &mut self.buf
        }
    }

    impl<'a> Encoder<'a> for AccountCommissionResponseEncoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> AccountCommissionResponseEncoder<'a> {
        pub fn wrap(mut self, buf: WriteBuf<'a>, offset: usize) -> Self {
            let limit = offset + SBE_BLOCK_LENGTH as usize;
            self.buf = buf;
            self.initial_offset = offset;
            self.offset = offset;
            self.limit = limit;
            self
        }

        #[inline]
        pub fn encoded_length(&self) -> usize {
            self.limit - self.offset
        }

        pub fn header(self, offset: usize) -> MessageHeaderEncoder<Self> {
            let mut header = MessageHeaderEncoder::default().wrap(self, offset);
            header.block_length(SBE_BLOCK_LENGTH);
            header.template_id(SBE_TEMPLATE_ID);
            header.schema_id(SBE_SCHEMA_ID);
            header.version(SBE_SCHEMA_VERSION);
            header
        }

        /// primitive field 'commissionExponent'
        /// - min value: -127
        /// - max value: 127
        /// - null value: -128_i8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 0
        /// - encodedLength: 1
        /// - version: 0
        #[inline]
        pub fn commission_exponent(&mut self, value: i8) {
            let offset = self.offset;
            self.get_buf_mut().put_i8_at(offset, value);
        }

        /// primitive field 'discountExponent'
        /// - min value: -127
        /// - max value: 127
        /// - null value: -128_i8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 1
        /// - encodedLength: 1
        /// - version: 0
        #[inline]
        pub fn discount_exponent(&mut self, value: i8) {
            let offset = self.offset + 1;
            self.get_buf_mut().put_i8_at(offset, value);
        }

        /// primitive field 'standardCommissionMaker'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 2
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn standard_commission_maker(&mut self, value: i64) {
            let offset = self.offset + 2;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'standardCommissionTaker'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 10
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn standard_commission_taker(&mut self, value: i64) {
            let offset = self.offset + 10;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'standardCommissionBuyer'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 18
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn standard_commission_buyer(&mut self, value: i64) {
            let offset = self.offset + 18;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'standardCommissionSeller'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 26
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn standard_commission_seller(&mut self, value: i64) {
            let offset = self.offset + 26;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'taxCommissionMaker'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 34
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn tax_commission_maker(&mut self, value: i64) {
            let offset = self.offset + 34;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'taxCommissionTaker'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 42
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn tax_commission_taker(&mut self, value: i64) {
            let offset = self.offset + 42;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'taxCommissionBuyer'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 50
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn tax_commission_buyer(&mut self, value: i64) {
            let offset = self.offset + 50;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'taxCommissionSeller'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 58
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn tax_commission_seller(&mut self, value: i64) {
            let offset = self.offset + 58;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// REQUIRED enum
        #[inline]
        pub fn discount_enabled_for_account(&mut self, value: bool_enum::BoolEnum) {
            let offset = self.offset + 66;
            self.get_buf_mut().put_u8_at(offset, value as u8)
        }

        /// REQUIRED enum
        #[inline]
        pub fn discount_enabled_for_symbol(&mut self, value: bool_enum::BoolEnum) {
            let offset = self.offset + 67;
            self.get_buf_mut().put_u8_at(offset, value as u8)
        }

        /// primitive field 'discount'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 68
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn discount(&mut self, value: i64) {
            let offset = self.offset + 68;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'specialCommissionMaker'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 76
        /// - encodedLength: 8
        /// - version: 1
        #[inline]
        pub fn special_commission_maker(&mut self, value: i64) {
            let offset = self.offset + 76;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'specialCommissionTaker'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 84
        /// - encodedLength: 8
        /// - version: 1
        #[inline]
        pub fn special_commission_taker(&mut self, value: i64) {
            let offset = self.offset + 84;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'specialCommissionBuyer'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 92
        /// - encodedLength: 8
        /// - version: 1
        #[inline]
        pub fn special_commission_buyer(&mut self, value: i64) {
            let offset = self.offset + 92;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'specialCommissionSeller'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 100
        /// - encodedLength: 8
        /// - version: 1
        #[inline]
        pub fn special_commission_seller(&mut self, value: i64) {
            let offset = self.offset + 100;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// VAR_DATA ENCODER - character encoding: 'UTF-8'
        #[inline]
        pub fn symbol(&mut self, value: &str) {
            let limit = self.get_limit();
            let data_length = value.len();
            self.set_limit(limit + 1 + data_length);
            self.get_buf_mut().put_u8_at(limit, data_length as u8);
            self.get_buf_mut().put_slice_at(limit + 1, value.as_bytes());
        }

        /// VAR_DATA ENCODER - character encoding: 'UTF-8'
        #[inline]
        pub fn discount_asset(&mut self, value: &str) {
            let limit = self.get_limit();
            let data_length = value.len();
            self.set_limit(limit + 1 + data_length);
            self.get_buf_mut().put_u8_at(limit, data_length as u8);
            self.get_buf_mut().put_slice_at(limit + 1, value.as_bytes());
        }
    }
} // end encoder

pub mod decoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct AccountCommissionResponseDecoder<'a> {
        buf: ReadBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
        pub acting_block_length: u16,
        pub acting_version: u16,
    }

    impl ActingVersion for AccountCommissionResponseDecoder<'_> {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.acting_version
        }
    }

    impl<'a> Reader<'a> for AccountCommissionResponseDecoder<'a> {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            &self.buf
        }
    }

    impl<'a> Decoder<'a> for AccountCommissionResponseDecoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> AccountCommissionResponseDecoder<'a> {
        pub fn wrap(
            mut self,
            buf: ReadBuf<'a>,
            offset: usize,
            acting_block_length: u16,
            acting_version: u16,
        ) -> Self {
            let limit = offset + acting_block_length as usize;
            self.buf = buf;
            self.initial_offset = offset;
            self.offset = offset;
            self.limit = limit;
            self.acting_block_length = acting_block_length;
            self.acting_version = acting_version;
            self
        }

        #[inline]
        pub fn encoded_length(&self) -> usize {
            self.limit - self.offset
        }

        pub fn header(self, mut header: MessageHeaderDecoder<ReadBuf<'a>>, offset: usize) -> Self {
            debug_assert_eq!(SBE_TEMPLATE_ID, header.template_id());
            let acting_block_length = header.block_length();
            let acting_version = header.version();

            self.wrap(
                header.parent().unwrap(),
                offset + message_header_codec::ENCODED_LENGTH,
                acting_block_length,
                acting_version,
            )
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn commission_exponent(&self) -> i8 {
            self.get_buf().get_i8_at(self.offset)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn discount_exponent(&self) -> i8 {
            self.get_buf().get_i8_at(self.offset + 1)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn standard_commission_maker(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 2)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn standard_commission_taker(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 10)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn standard_commission_buyer(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 18)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn standard_commission_seller(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 26)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn tax_commission_maker(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 34)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn tax_commission_taker(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 42)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn tax_commission_buyer(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 50)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn tax_commission_seller(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 58)
        }

        /// REQUIRED enum
        #[inline]
        pub fn discount_enabled_for_account(&self) -> bool_enum::BoolEnum {
            self.get_buf().get_u8_at(self.offset + 66).into()
        }

        /// REQUIRED enum
        #[inline]
        pub fn discount_enabled_for_symbol(&self) -> bool_enum::BoolEnum {
            self.get_buf().get_u8_at(self.offset + 67).into()
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn discount(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 68)
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn special_commission_maker(&self) -> Option<i64> {
            if self.acting_version() < 1 {
                return None;
            }

            let value = self.get_buf().get_i64_at(self.offset + 76);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn special_commission_taker(&self) -> Option<i64> {
            if self.acting_version() < 1 {
                return None;
            }

            let value = self.get_buf().get_i64_at(self.offset + 84);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn special_commission_buyer(&self) -> Option<i64> {
            if self.acting_version() < 1 {
                return None;
            }

            let value = self.get_buf().get_i64_at(self.offset + 92);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn special_commission_seller(&self) -> Option<i64> {
            if self.acting_version() < 1 {
                return None;
            }

            let value = self.get_buf().get_i64_at(self.offset + 100);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// VAR_DATA DECODER - character encoding: 'UTF-8'
        #[inline]
        pub fn symbol_decoder(&mut self) -> (usize, usize) {
            let offset = self.get_limit();
            let data_length = self.get_buf().get_u8_at(offset) as usize;
            self.set_limit(offset + 1 + data_length);
            (offset + 1, data_length)
        }

        #[inline]
        pub fn symbol_slice(&'a self, coordinates: (usize, usize)) -> &'a [u8] {
            debug_assert!(self.get_limit() >= coordinates.0 + coordinates.1);
            self.get_buf().get_slice_at(coordinates.0, coordinates.1)
        }

        /// VAR_DATA DECODER - character encoding: 'UTF-8'
        #[inline]
        pub fn discount_asset_decoder(&mut self) -> (usize, usize) {
            let offset = self.get_limit();
            let data_length = self.get_buf().get_u8_at(offset) as usize;
            self.set_limit(offset + 1 + data_length);
            (offset + 1, data_length)
        }

        #[inline]
        pub fn discount_asset_slice(&'a self, coordinates: (usize, usize)) -> &'a [u8] {
            debug_assert!(self.get_limit() >= coordinates.0 + coordinates.1);
            self.get_buf().get_slice_at(coordinates.0, coordinates.1)
        }
    }
} // end decoder
