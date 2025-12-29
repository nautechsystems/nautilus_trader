pub use decoder::CancelReplaceOrderResponseDecoder;
pub use encoder::CancelReplaceOrderResponseEncoder;

use super::*;
pub use super::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SBE_SEMANTIC_VERSION};

pub const SBE_BLOCK_LENGTH: u16 = 2;
pub const SBE_TEMPLATE_ID: u16 = 307;

pub mod encoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Debug, Default)]
    pub struct CancelReplaceOrderResponseEncoder<'a> {
        buf: WriteBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
    }

    impl<'a> Writer<'a> for CancelReplaceOrderResponseEncoder<'a> {
        #[inline]
        fn get_buf_mut(&mut self) -> &mut WriteBuf<'a> {
            &mut self.buf
        }
    }

    impl<'a> Encoder<'a> for CancelReplaceOrderResponseEncoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> CancelReplaceOrderResponseEncoder<'a> {
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

        /// REQUIRED enum
        #[inline]
        pub fn cancel_result(&mut self, value: cancel_replace_status::CancelReplaceStatus) {
            let offset = self.offset;
            self.get_buf_mut().put_u8_at(offset, value as u8)
        }

        /// REQUIRED enum
        #[inline]
        pub fn new_order_result(&mut self, value: cancel_replace_status::CancelReplaceStatus) {
            let offset = self.offset + 1;
            self.get_buf_mut().put_u8_at(offset, value as u8)
        }

        /// VAR_DATA ENCODER - character encoding: 'None'
        #[inline]
        pub fn cancel_response(&mut self, value: &[u8]) {
            let limit = self.get_limit();
            let data_length = value.len();
            self.set_limit(limit + 2 + data_length);
            self.get_buf_mut().put_u16_at(limit, data_length as u16);
            self.get_buf_mut().put_slice_at(limit + 2, value);
        }

        /// VAR_DATA ENCODER - character encoding: 'None'
        #[inline]
        pub fn new_order_response(&mut self, value: &[u8]) {
            let limit = self.get_limit();
            let data_length = value.len();
            self.set_limit(limit + 4 + data_length);
            self.get_buf_mut().put_u32_at(limit, data_length as u32);
            self.get_buf_mut().put_slice_at(limit + 4, value);
        }
    }
} // end encoder

pub mod decoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct CancelReplaceOrderResponseDecoder<'a> {
        buf: ReadBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
        pub acting_block_length: u16,
        pub acting_version: u16,
    }

    impl ActingVersion for CancelReplaceOrderResponseDecoder<'_> {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.acting_version
        }
    }

    impl<'a> Reader<'a> for CancelReplaceOrderResponseDecoder<'a> {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            &self.buf
        }
    }

    impl<'a> Decoder<'a> for CancelReplaceOrderResponseDecoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> CancelReplaceOrderResponseDecoder<'a> {
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

        /// REQUIRED enum
        #[inline]
        pub fn cancel_result(&self) -> cancel_replace_status::CancelReplaceStatus {
            self.get_buf().get_u8_at(self.offset).into()
        }

        /// REQUIRED enum
        #[inline]
        pub fn new_order_result(&self) -> cancel_replace_status::CancelReplaceStatus {
            self.get_buf().get_u8_at(self.offset + 1).into()
        }

        /// VAR_DATA DECODER - character encoding: 'None'
        #[inline]
        pub fn cancel_response_decoder(&mut self) -> (usize, usize) {
            let offset = self.get_limit();
            let data_length = self.get_buf().get_u16_at(offset) as usize;
            self.set_limit(offset + 2 + data_length);
            (offset + 2, data_length)
        }

        #[inline]
        pub fn cancel_response_slice(&'a self, coordinates: (usize, usize)) -> &'a [u8] {
            debug_assert!(self.get_limit() >= coordinates.0 + coordinates.1);
            self.get_buf().get_slice_at(coordinates.0, coordinates.1)
        }

        /// VAR_DATA DECODER - character encoding: 'None'
        #[inline]
        pub fn new_order_response_decoder(&mut self) -> (usize, usize) {
            let offset = self.get_limit();
            let data_length = self.get_buf().get_u32_at(offset) as usize;
            self.set_limit(offset + 4 + data_length);
            (offset + 4, data_length)
        }

        #[inline]
        pub fn new_order_response_slice(&'a self, coordinates: (usize, usize)) -> &'a [u8] {
            debug_assert!(self.get_limit() >= coordinates.0 + coordinates.1);
            self.get_buf().get_slice_at(coordinates.0, coordinates.1)
        }
    }
} // end decoder
