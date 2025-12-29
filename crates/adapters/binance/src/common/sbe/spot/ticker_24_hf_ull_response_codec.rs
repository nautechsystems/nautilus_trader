pub use decoder::Ticker24hFullResponseDecoder;
pub use encoder::Ticker24hFullResponseEncoder;

use super::*;
pub use super::{SBE_SCHEMA_ID, SBE_SCHEMA_VERSION, SBE_SEMANTIC_VERSION};

pub const SBE_BLOCK_LENGTH: u16 = 0;
pub const SBE_TEMPLATE_ID: u16 = 206;

pub mod encoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Debug, Default)]
    pub struct Ticker24hFullResponseEncoder<'a> {
        buf: WriteBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
    }

    impl<'a> Writer<'a> for Ticker24hFullResponseEncoder<'a> {
        #[inline]
        fn get_buf_mut(&mut self) -> &mut WriteBuf<'a> {
            &mut self.buf
        }
    }

    impl<'a> Encoder<'a> for Ticker24hFullResponseEncoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> Ticker24hFullResponseEncoder<'a> {
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

        /// GROUP ENCODER (id=100)
        #[inline]
        pub fn tickers_encoder(
            self,
            count: u32,
            tickers_encoder: TickersEncoder<Self>,
        ) -> TickersEncoder<Self> {
            tickers_encoder.wrap(self, count)
        }
    }

    #[derive(Debug, Default)]
    pub struct TickersEncoder<P> {
        parent: Option<P>,
        count: u32,
        index: usize,
        offset: usize,
        initial_limit: usize,
    }

    impl<'a, P> Writer<'a> for TickersEncoder<P>
    where
        P: Writer<'a> + Default,
    {
        #[inline]
        fn get_buf_mut(&mut self) -> &mut WriteBuf<'a> {
            if let Some(parent) = self.parent.as_mut() {
                parent.get_buf_mut()
            } else {
                panic!("parent was None")
            }
        }
    }

    impl<'a, P> Encoder<'a> for TickersEncoder<P>
    where
        P: Encoder<'a> + Default,
    {
        #[inline]
        fn get_limit(&self) -> usize {
            self.parent.as_ref().expect("parent missing").get_limit()
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.parent
                .as_mut()
                .expect("parent missing")
                .set_limit(limit);
        }
    }

    impl<'a, P> TickersEncoder<P>
    where
        P: Encoder<'a> + Default,
    {
        #[inline]
        pub fn wrap(mut self, mut parent: P, count: u32) -> Self {
            let initial_limit = parent.get_limit();
            parent.set_limit(initial_limit + 6);
            parent
                .get_buf_mut()
                .put_u16_at(initial_limit, Self::block_length());
            parent.get_buf_mut().put_u32_at(initial_limit + 2, count);
            self.parent = Some(parent);
            self.count = count;
            self.index = usize::MAX;
            self.offset = usize::MAX;
            self.initial_limit = initial_limit;
            self
        }

        #[inline]
        pub fn block_length() -> u16 {
            182
        }

        #[inline]
        pub fn parent(&mut self) -> SbeResult<P> {
            self.parent.take().ok_or(SbeErr::ParentNotSet)
        }

        /// will return Some(current index) when successful otherwise None
        #[inline]
        pub fn advance(&mut self) -> SbeResult<Option<usize>> {
            let index = self.index.wrapping_add(1);
            if index >= self.count as usize {
                return Ok(None);
            }
            if let Some(parent) = self.parent.as_mut() {
                self.offset = parent.get_limit();
                parent.set_limit(self.offset + Self::block_length() as usize);
                self.index = index;
                Ok(Some(index))
            } else {
                Err(SbeErr::ParentNotSet)
            }
        }

        /// primitive field 'priceExponent'
        /// - min value: -127
        /// - max value: 127
        /// - null value: -128_i8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 0
        /// - encodedLength: 1
        /// - version: 0
        #[inline]
        pub fn price_exponent(&mut self, value: i8) {
            let offset = self.offset;
            self.get_buf_mut().put_i8_at(offset, value);
        }

        /// primitive field 'qtyExponent'
        /// - min value: -127
        /// - max value: 127
        /// - null value: -128_i8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 1
        /// - encodedLength: 1
        /// - version: 0
        #[inline]
        pub fn qty_exponent(&mut self, value: i8) {
            let offset = self.offset + 1;
            self.get_buf_mut().put_i8_at(offset, value);
        }

        /// primitive field 'priceChange'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 2
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn price_change(&mut self, value: i64) {
            let offset = self.offset + 2;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'priceChangePercent'
        /// - min value: -3.4028234663852886E38
        /// - max value: 3.4028234663852886E38
        /// - null value: f32::NAN
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 10
        /// - encodedLength: 4
        /// - version: 0
        #[inline]
        pub fn price_change_percent(&mut self, value: f32) {
            let offset = self.offset + 10;
            self.get_buf_mut().put_f32_at(offset, value);
        }

        /// primitive field 'weightedAvgPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 14
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn weighted_avg_price(&mut self, value: i64) {
            let offset = self.offset + 14;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'prevClosePrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 22
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn prev_close_price(&mut self, value: i64) {
            let offset = self.offset + 22;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'lastPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 30
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn last_price(&mut self, value: i64) {
            let offset = self.offset + 30;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        #[inline]
        pub fn last_qty_at(&mut self, index: usize, value: u8) {
            let offset = self.offset + 38;
            let buf = self.get_buf_mut();
            buf.put_u8_at(offset + index, value);
        }

        /// primitive array field 'lastQty'
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 38
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn last_qty(&mut self, value: &[u8]) {
            debug_assert_eq!(16, value.len());
            let offset = self.offset + 38;
            let buf = self.get_buf_mut();
            buf.put_slice_at(offset, value);
        }

        /// primitive array field 'lastQty' from an Iterator
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 38
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn last_qty_from_iter(&mut self, iter: impl Iterator<Item = u8>) {
            let offset = self.offset + 38;
            let buf = self.get_buf_mut();
            for (i, v) in iter.enumerate() {
                buf.put_u8_at(offset + i, v);
            }
        }

        /// primitive array field 'lastQty' with zero padding
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 38
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn last_qty_zero_padded(&mut self, value: &[u8]) {
            let iter = value
                .iter()
                .copied()
                .chain(std::iter::repeat(0_u8))
                .take(16);
            self.last_qty_from_iter(iter);
        }

        /// primitive field 'bidPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 54
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn bid_price(&mut self, value: i64) {
            let offset = self.offset + 54;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'bidQty'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 62
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn bid_qty(&mut self, value: i64) {
            let offset = self.offset + 62;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'askPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 70
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn ask_price(&mut self, value: i64) {
            let offset = self.offset + 70;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'askQty'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 78
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn ask_qty(&mut self, value: i64) {
            let offset = self.offset + 78;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'openPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 86
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn open_price(&mut self, value: i64) {
            let offset = self.offset + 86;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'highPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 94
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn high_price(&mut self, value: i64) {
            let offset = self.offset + 94;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'lowPrice'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 102
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn low_price(&mut self, value: i64) {
            let offset = self.offset + 102;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        #[inline]
        pub fn volume_at(&mut self, index: usize, value: u8) {
            let offset = self.offset + 110;
            let buf = self.get_buf_mut();
            buf.put_u8_at(offset + index, value);
        }

        /// primitive array field 'volume'
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 110
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn volume(&mut self, value: &[u8]) {
            debug_assert_eq!(16, value.len());
            let offset = self.offset + 110;
            let buf = self.get_buf_mut();
            buf.put_slice_at(offset, value);
        }

        /// primitive array field 'volume' from an Iterator
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 110
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn volume_from_iter(&mut self, iter: impl Iterator<Item = u8>) {
            let offset = self.offset + 110;
            let buf = self.get_buf_mut();
            for (i, v) in iter.enumerate() {
                buf.put_u8_at(offset + i, v);
            }
        }

        /// primitive array field 'volume' with zero padding
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 110
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn volume_zero_padded(&mut self, value: &[u8]) {
            let iter = value
                .iter()
                .copied()
                .chain(std::iter::repeat(0_u8))
                .take(16);
            self.volume_from_iter(iter);
        }

        #[inline]
        pub fn quote_volume_at(&mut self, index: usize, value: u8) {
            let offset = self.offset + 126;
            let buf = self.get_buf_mut();
            buf.put_u8_at(offset + index, value);
        }

        /// primitive array field 'quoteVolume'
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 126
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn quote_volume(&mut self, value: &[u8]) {
            debug_assert_eq!(16, value.len());
            let offset = self.offset + 126;
            let buf = self.get_buf_mut();
            buf.put_slice_at(offset, value);
        }

        /// primitive array field 'quoteVolume' from an Iterator
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 126
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn quote_volume_from_iter(&mut self, iter: impl Iterator<Item = u8>) {
            let offset = self.offset + 126;
            let buf = self.get_buf_mut();
            for (i, v) in iter.enumerate() {
                buf.put_u8_at(offset + i, v);
            }
        }

        /// primitive array field 'quoteVolume' with zero padding
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 126
        /// - encodedLength: 16
        /// - version: 0
        #[inline]
        pub fn quote_volume_zero_padded(&mut self, value: &[u8]) {
            let iter = value
                .iter()
                .copied()
                .chain(std::iter::repeat(0_u8))
                .take(16);
            self.quote_volume_from_iter(iter);
        }

        /// primitive field 'openTime'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 142
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn open_time(&mut self, value: i64) {
            let offset = self.offset + 142;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'closeTime'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 150
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn close_time(&mut self, value: i64) {
            let offset = self.offset + 150;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'firstId'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 158
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn first_id(&mut self, value: i64) {
            let offset = self.offset + 158;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'lastId'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 166
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn last_id(&mut self, value: i64) {
            let offset = self.offset + 166;
            self.get_buf_mut().put_i64_at(offset, value);
        }

        /// primitive field 'numTrades'
        /// - min value: -9223372036854775807
        /// - max value: 9223372036854775807
        /// - null value: -9223372036854775808_i64
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 174
        /// - encodedLength: 8
        /// - version: 0
        #[inline]
        pub fn num_trades(&mut self, value: i64) {
            let offset = self.offset + 174;
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
    }
} // end encoder

pub mod decoder {
    use message_header_codec::*;

    use super::*;

    #[derive(Clone, Copy, Debug, Default)]
    pub struct Ticker24hFullResponseDecoder<'a> {
        buf: ReadBuf<'a>,
        initial_offset: usize,
        offset: usize,
        limit: usize,
        pub acting_block_length: u16,
        pub acting_version: u16,
    }

    impl ActingVersion for Ticker24hFullResponseDecoder<'_> {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.acting_version
        }
    }

    impl<'a> Reader<'a> for Ticker24hFullResponseDecoder<'a> {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            &self.buf
        }
    }

    impl<'a> Decoder<'a> for Ticker24hFullResponseDecoder<'a> {
        #[inline]
        fn get_limit(&self) -> usize {
            self.limit
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.limit = limit;
        }
    }

    impl<'a> Ticker24hFullResponseDecoder<'a> {
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

        /// GROUP DECODER (id=100)
        #[inline]
        pub fn tickers_decoder(self) -> TickersDecoder<Self> {
            TickersDecoder::default().wrap(self)
        }
    }

    #[derive(Debug, Default)]
    pub struct TickersDecoder<P> {
        parent: Option<P>,
        block_length: u16,
        count: u32,
        index: usize,
        offset: usize,
    }

    impl<'a, P> ActingVersion for TickersDecoder<P>
    where
        P: Reader<'a> + ActingVersion + Default,
    {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.parent.as_ref().unwrap().acting_version()
        }
    }

    impl<'a, P> Reader<'a> for TickersDecoder<P>
    where
        P: Reader<'a> + Default,
    {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            self.parent.as_ref().expect("parent missing").get_buf()
        }
    }

    impl<'a, P> Decoder<'a> for TickersDecoder<P>
    where
        P: Decoder<'a> + ActingVersion + Default,
    {
        #[inline]
        fn get_limit(&self) -> usize {
            self.parent.as_ref().expect("parent missing").get_limit()
        }

        #[inline]
        fn set_limit(&mut self, limit: usize) {
            self.parent
                .as_mut()
                .expect("parent missing")
                .set_limit(limit);
        }
    }

    impl<'a, P> TickersDecoder<P>
    where
        P: Decoder<'a> + ActingVersion + Default,
    {
        pub fn wrap(mut self, mut parent: P) -> Self {
            let initial_offset = parent.get_limit();
            let block_length = parent.get_buf().get_u16_at(initial_offset);
            let count = parent.get_buf().get_u32_at(initial_offset + 2);
            parent.set_limit(initial_offset + 6);
            self.parent = Some(parent);
            self.block_length = block_length;
            self.count = count;
            self.index = usize::MAX;
            self.offset = 0;
            self
        }

        /// group token - Token{signal=BEGIN_GROUP, name='tickers', referencedName='null', description='null', packageName='null', id=100, version=0, deprecated=0, encodedLength=182, offset=0, componentTokenCount=78, encoding=Encoding{presence=REQUIRED, primitiveType=null, byteOrder=LITTLE_ENDIAN, minValue=null, maxValue=null, nullValue=null, constValue=null, characterEncoding='null', epoch='null', timeUnit=null, semanticType='null'}}
        #[inline]
        pub fn parent(&mut self) -> SbeResult<P> {
            self.parent.take().ok_or(SbeErr::ParentNotSet)
        }

        #[inline]
        pub fn acting_version(&mut self) -> u16 {
            self.parent.as_ref().unwrap().acting_version()
        }

        #[inline]
        pub fn count(&self) -> u32 {
            self.count
        }

        /// will return Some(current index) when successful otherwise None
        pub fn advance(&mut self) -> SbeResult<Option<usize>> {
            let index = self.index.wrapping_add(1);
            if index >= self.count as usize {
                return Ok(None);
            }
            if let Some(parent) = self.parent.as_mut() {
                self.offset = parent.get_limit();
                parent.set_limit(self.offset + self.block_length as usize);
                self.index = index;
                Ok(Some(index))
            } else {
                Err(SbeErr::ParentNotSet)
            }
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn price_exponent(&self) -> i8 {
            self.get_buf().get_i8_at(self.offset)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn qty_exponent(&self) -> i8 {
            self.get_buf().get_i8_at(self.offset + 1)
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn price_change(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 2);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: 'f32::NAN' }
        #[inline]
        pub fn price_change_percent(&self) -> Option<f32> {
            let value = self.get_buf().get_f32_at(self.offset + 10);
            if value.is_nan() { None } else { Some(value) }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn weighted_avg_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 14);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn prev_close_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 22);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn last_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 30);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        #[inline]
        pub fn last_qty(&self) -> [u8; 16] {
            let buf = self.get_buf();
            ReadBuf::get_bytes_at(buf.data, self.offset + 38)
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn bid_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 54);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn bid_qty(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 62)
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn ask_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 70);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn ask_qty(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 78)
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn open_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 86);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn high_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 94);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn low_price(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 102);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        #[inline]
        pub fn volume(&self) -> [u8; 16] {
            let buf = self.get_buf();
            ReadBuf::get_bytes_at(buf.data, self.offset + 110)
        }

        #[inline]
        pub fn quote_volume(&self) -> [u8; 16] {
            let buf = self.get_buf();
            ReadBuf::get_bytes_at(buf.data, self.offset + 126)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn open_time(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 142)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn close_time(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 150)
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn first_id(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 158);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'OPTIONAL' { null_value: '-9223372036854775808_i64' }
        #[inline]
        pub fn last_id(&self) -> Option<i64> {
            let value = self.get_buf().get_i64_at(self.offset + 166);
            if value == -9223372036854775808_i64 {
                None
            } else {
                Some(value)
            }
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn num_trades(&self) -> i64 {
            self.get_buf().get_i64_at(self.offset + 174)
        }

        /// VAR_DATA DECODER - character encoding: 'UTF-8'
        #[inline]
        pub fn symbol_decoder(&mut self) -> (usize, usize) {
            let offset = self.parent.as_ref().expect("parent missing").get_limit();
            let data_length = self.get_buf().get_u8_at(offset) as usize;
            self.parent
                .as_mut()
                .unwrap()
                .set_limit(offset + 1 + data_length);
            (offset + 1, data_length)
        }

        #[inline]
        pub fn symbol_slice(&'a self, coordinates: (usize, usize)) -> &'a [u8] {
            debug_assert!(self.get_limit() >= coordinates.0 + coordinates.1);
            self.get_buf().get_slice_at(coordinates.0, coordinates.1)
        }
    }
} // end decoder
