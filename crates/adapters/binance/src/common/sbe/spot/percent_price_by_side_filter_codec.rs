pub use decoder::PercentPriceBySideFilterDecoder;
pub use encoder::PercentPriceBySideFilterEncoder;

use super::*;
pub use super::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SBE_SEMANTIC_VERSION};

pub const SBE_BLOCK_LENGTH: u16 = 37;
pub const SBE_TEMPLATE_ID: u16 = 3;

pub mod encoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Debug, Default)]
    pub struct PercentPriceBySideFilterEncoder<'a> {
        buf: WriteBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
    }

    impl<'a> Writer<'a> for PercentPriceBySideFilterEncoder<'a> {
        #[inline]
        fn get_buf_mut(&mut self) -> &mut WriteBuf<'a> {
            &mut self.buf
        }
    }

    impl<'a> Encoder<'a> for PercentPriceBySideFilterEncoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> PercentPriceBySideFilterEncoder<'a> {
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

        // skipping CONSTANT enum 'filterType'

        /// primitive field 'multiplierExponent'
        /// - min value: -127
        /// - max value: 127
        /// - null value: -128_i8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 0
        /// - encodedLength: 1
        /// - version: 0
        #[inline]
        pub fn multiplier_exponent(&mut self, value: i8) {
            let offset = self.offset;
            self.get_buf_mut().put_i8_at(offset, value);
        }

        /// primitive field 'bidMultiplierUp'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 1
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn bid_multiplier_up(&mut self, value: i64) {
            let offset = self.offset + 1;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'bidMultiplierDown'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 9
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn bid_multiplier_down(&mut self, value: i64) {
            let offset = self.offset + 9;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'askMultiplierUp'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 17
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn ask_multiplier_up(&mut self, value: i64) {
            let offset = self.offset + 17;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'askMultiplierDown'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 25
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn ask_multiplier_down(&mut self, value: i64) {
            let offset = self.offset + 25;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'avgPriceMins'
        /// - min value: -2147483647
        /// - max value: 2147483647
        /// - null value: -2147483648_i32
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 33
        /// - encodedLength: 4
        /// - version: 0
        #[inline]
        pub fn avg_price_mins(&mut self, value: i32) {
            let offset = self.offset + 33;
            self.get_buf_mut().put_i32_at(offset, value);
        }
    }
} // end encoder

pub mod decoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct PercentPriceBySideFilterDecoder<'a> {
        buf: ReadBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
        pub acting_block_length: u16,
        pub acting_version: u16,
    }

    impl ActingVersion for PercentPriceBySideFilterDecoder<'_> {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.acting_version
        }
    }

    impl<'a> Reader<'a> for PercentPriceBySideFilterDecoder<'a> {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            &self.buf
        }
    }

    impl<'a> Decoder<'a> for PercentPriceBySideFilterDecoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> PercentPriceBySideFilterDecoder<'a> {
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

        /// CONSTANT enum
        #[inline]
        pub fn filter_type(&self) -> filter_type::FilterType {
            filter_type::FilterType::PercentPriceBySide
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn multiplier_exponent(&self) -> i8 {
            self.get_buf().get_i8_at(self.offset)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn bid_multiplier_up(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 1)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn bid_multiplier_down(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 9)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn ask_multiplier_up(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 17)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn ask_multiplier_down(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 25)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn avg_price_mins(&self) -> i32 {
            self.get_buf().get_i32_at(self.offset + 33)
        }
    }
} // end decoder
