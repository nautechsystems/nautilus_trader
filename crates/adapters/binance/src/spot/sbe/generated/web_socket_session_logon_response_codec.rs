pub use decoder::WebSocketSessionLogonResponseDecoder;
pub use encoder::WebSocketSessionLogonResponseEncoder;

use super::*;
pub use super::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SBE_SEMANTIC_VERSION};

pub const SBE_BLOCK_LENGTH: u16 = 26;
pub const SBE_TEMPLATE_ID: u16 = 51;

pub mod encoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Debug, Default)]
    pub struct WebSocketSessionLogonResponseEncoder<'a> {
        buf: WriteBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
    }

    impl<'a> Writer<'a> for WebSocketSessionLogonResponseEncoder<'a> {
        #[inline]
        fn get_buf_mut(&mut self) -> &mut WriteBuf<'a> {
            &mut self.buf
        }
    }

    impl<'a> Encoder<'a> for WebSocketSessionLogonResponseEncoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> WebSocketSessionLogonResponseEncoder<'a> {
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

        /// primitive field 'authorizedSince'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 0
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn authorized_since(&mut self, value: i64) {
            let offset = self.offset;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'connectedSince'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 8
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn connected_since(&mut self, value: i64) {
            let offset = self.offset + 8;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// REQUIRED enum
        #[inline]
        pub fn return_rate_limits(&mut self, value: bool_enum::BoolEnum) {
            let offset = self.offset + 16;
            self.get_buf_mut().put_u8_at(offset, value as u8)
        }

        /// primitive field 'serverTime'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 17
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn server_time(&mut self, value: i64) {
            let offset = self.offset + 17;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// REQUIRED enum
        #[inline]
        pub fn user_data_stream(&mut self, value: bool_enum::BoolEnum) {
            let offset = self.offset + 25;
            self.get_buf_mut().put_u8_at(offset, value as u8)
        }

        /// VAR_DATA ENCODER - character encoding: 'UTF-8'
        #[inline]
        pub fn logged_on_api_key(&mut self, value: &str) {
            let limit = self.get_limit();
            let data_length = value.len();
            self.set_limit(limit + 2 + data_length);
            self.get_buf_mut().put_u16_at(limit, data_length as u16);
            self.get_buf_mut().put_slice_at(limit + 2, value.as_bytes());
        }
    }
} // end encoder

pub mod decoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct WebSocketSessionLogonResponseDecoder<'a> {
        buf: ReadBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
        pub acting_block_length: u16,
        pub acting_version: u16,
    }

    impl ActingVersion for WebSocketSessionLogonResponseDecoder<'_> {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.acting_version
        }
    }

    impl<'a> Reader<'a> for WebSocketSessionLogonResponseDecoder<'a> {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            &self.buf
        }
    }

    impl<'a> Decoder<'a> for WebSocketSessionLogonResponseDecoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> WebSocketSessionLogonResponseDecoder<'a> {
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
        pub fn authorized_since(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn connected_since(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 8)
        }

        /// REQUIRED enum
        #[inline]
        pub fn return_rate_limits(&self) -> bool_enum::BoolEnum {
            self.get_buf().get_u8_at(self.offset + 16).into()
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn server_time(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 17)
        }

        /// REQUIRED enum
        #[inline]
        pub fn user_data_stream(&self) -> bool_enum::BoolEnum {
            self.get_buf().get_u8_at(self.offset + 25).into()
        }

        /// VAR_DATA DECODER - character encoding: 'UTF-8'
        #[inline]
        pub fn logged_on_api_key_decoder(&mut self) -> (usize, usize) {
            let offset = self.get_limit();
            let data_length = self.get_buf().get_u16_at(offset) as usize;
            self.set_limit(offset + 2 + data_length);
            (offset + 2, data_length)
        }

        #[inline]
        pub fn logged_on_api_key_slice(&'a self, coordinates: (usize, usize)) -> &'a [u8] {
            debug_assert!(self.get_limit() >= coordinates.0 + coordinates.1);
            self.get_buf().get_slice_at(coordinates.0, coordinates.1)
        }
    }
} // end decoder
