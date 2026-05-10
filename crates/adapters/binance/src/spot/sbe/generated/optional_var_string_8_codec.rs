pub use decoder::OptionalVarString8Decoder;
pub use encoder::OptionalVarString8Encoder;

use super::*;

pub mod encoder {
    use super::*;

    #[derive(Debug, Default)]
    pub struct OptionalVarString8Encoder<P> {
        parent: Option<P>,
        offset: usize,
    }

    impl<'a, P> Writer<'a> for OptionalVarString8Encoder<P>
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

    impl<'a, P> OptionalVarString8Encoder<P>
    where
        P: Writer<'a> + Default,
    {
        pub fn wrap(mut self, parent: P, offset: usize) -> Self {
            self.parent = Some(parent);
            self.offset = offset;
            self
        }

        #[inline]
        pub fn parent(&mut self) -> SbeResult<P> {
            self.parent.take().ok_or(SbeErr::ParentNotSet)
        }

        /// primitive field 'length'
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: null
        /// - semanticType: null
        /// - encodedOffset: 0
        /// - encodedLength: 1
        /// - version: 0
        #[inline]
        pub fn length(&mut self, value: u8) {
            let offset = self.offset;
            self.get_buf_mut().put_u8_at(offset, value);
        }

        /// primitive field 'varData'
        /// - min value: 0
        /// - max value: 254
        /// - null value: 0xff_u8
        /// - characterEncoding: UTF-8
        /// - semanticType: null
        /// - encodedOffset: 1
        /// - encodedLength: -1
        /// - version: 0
        #[inline]
        pub fn var_data(&mut self, value: u8) {
            let offset = self.offset + 1;
            self.get_buf_mut().put_u8_at(offset, value);
        }
    }
} // end encoder mod

pub mod decoder {
    use super::*;

    #[derive(Debug, Default)]
    pub struct OptionalVarString8Decoder<P> {
        parent: Option<P>,
        offset: usize,
    }

    impl<'a, P> ActingVersion for OptionalVarString8Decoder<P>
    where
        P: Reader<'a> + ActingVersion + Default,
    {
        #[inline]
        fn acting_version(&self) -> u16 {
            self.parent.as_ref().unwrap().acting_version()
        }
    }

    impl<'a, P> Reader<'a> for OptionalVarString8Decoder<P>
    where
        P: Reader<'a> + Default,
    {
        #[inline]
        fn get_buf(&self) -> &ReadBuf<'a> {
            self.parent.as_ref().expect("parent missing").get_buf()
        }
    }

    impl<'a, P> OptionalVarString8Decoder<P>
    where
        P: Reader<'a> + Default,
    {
        pub fn wrap(mut self, parent: P, offset: usize) -> Self {
            self.parent = Some(parent);
            self.offset = offset;
            self
        }

        #[inline]
        pub fn parent(&mut self) -> SbeResult<P> {
            self.parent.take().ok_or(SbeErr::ParentNotSet)
        }

        /// primitive field - 'REQUIRED'
        #[inline]
        pub fn length(&self) -> u8 {
            self.get_buf().get_u8_at(self.offset)
        }

        /// primitive field - 'REQUIRED'
        /// characterEncoding: 'UTF-8'
        #[inline]
        pub fn var_data(&self) -> u8 {
            self.get_buf().get_u8_at(self.offset + 1)
        }
    }
} // end decoder mod
