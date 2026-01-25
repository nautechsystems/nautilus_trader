#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct AllowedSelfTradePreventionModes(pub u8);
impl AllowedSelfTradePreventionModes {
    #[inline]
    pub fn new(value: u8) -> Self {
        AllowedSelfTradePreventionModes(value)
    }

    #[inline]
    pub fn clear(&mut self) -> &mut Self {
        self.0 = 0;
        self
    }

    #[inline]
    pub fn get_none(&self) -> bool {
        0 != self.0 & (1 << 0)
    }

    #[inline]
    pub fn set_none(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 0)
        } else {
            self.0 & !(1 << 0)
        };
        self
    }

    #[inline]
    pub fn get_expire_taker(&self) -> bool {
        0 != self.0 & (1 << 1)
    }

    #[inline]
    pub fn set_expire_taker(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 1)
        } else {
            self.0 & !(1 << 1)
        };
        self
    }

    #[inline]
    pub fn get_expire_maker(&self) -> bool {
        0 != self.0 & (1 << 2)
    }

    #[inline]
    pub fn set_expire_maker(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 2)
        } else {
            self.0 & !(1 << 2)
        };
        self
    }

    #[inline]
    pub fn get_expire_both(&self) -> bool {
        0 != self.0 & (1 << 3)
    }

    #[inline]
    pub fn set_expire_both(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 3)
        } else {
            self.0 & !(1 << 3)
        };
        self
    }

    #[inline]
    pub fn get_decrement(&self) -> bool {
        0 != self.0 & (1 << 4)
    }

    #[inline]
    pub fn set_decrement(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 4)
        } else {
            self.0 & !(1 << 4)
        };
        self
    }

    #[inline]
    pub fn get_non_representable(&self) -> bool {
        0 != self.0 & (1 << 7)
    }

    #[inline]
    pub fn set_non_representable(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 7)
        } else {
            self.0 & !(1 << 7)
        };
        self
    }
}
impl core::fmt::Debug for AllowedSelfTradePreventionModes {
    #[inline]
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            fmt,
            "AllowedSelfTradePreventionModes[none(0)={},expire_taker(1)={},expire_maker(2)={},expire_both(3)={},decrement(4)={},non_representable(7)={}]",
            self.get_none(),
            self.get_expire_taker(),
            self.get_expire_maker(),
            self.get_expire_both(),
            self.get_decrement(),
            self.get_non_representable(),
        )
    }
}
