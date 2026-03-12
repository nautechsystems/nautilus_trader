#[derive(Default, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OrderTypes(pub u16);
impl OrderTypes {
    #[inline]
    pub fn new(value: u16) -> Self {
        OrderTypes(value)
    }

    #[inline]
    pub fn clear(&mut self) -> &mut Self {
        self.0 = 0;
        self
    }

    #[inline]
    pub fn get_market(&self) -> bool {
        0 != self.0 & (1 << 0)
    }

    #[inline]
    pub fn set_market(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 0)
        } else {
            self.0 & !(1 << 0)
        };
        self
    }

    #[inline]
    pub fn get_limit(&self) -> bool {
        0 != self.0 & (1 << 1)
    }

    #[inline]
    pub fn set_limit(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 1)
        } else {
            self.0 & !(1 << 1)
        };
        self
    }

    #[inline]
    pub fn get_stop_loss(&self) -> bool {
        0 != self.0 & (1 << 2)
    }

    #[inline]
    pub fn set_stop_loss(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 2)
        } else {
            self.0 & !(1 << 2)
        };
        self
    }

    #[inline]
    pub fn get_stop_loss_limit(&self) -> bool {
        0 != self.0 & (1 << 3)
    }

    #[inline]
    pub fn set_stop_loss_limit(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 3)
        } else {
            self.0 & !(1 << 3)
        };
        self
    }

    #[inline]
    pub fn get_take_profit(&self) -> bool {
        0 != self.0 & (1 << 4)
    }

    #[inline]
    pub fn set_take_profit(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 4)
        } else {
            self.0 & !(1 << 4)
        };
        self
    }

    #[inline]
    pub fn get_take_profit_limit(&self) -> bool {
        0 != self.0 & (1 << 5)
    }

    #[inline]
    pub fn set_take_profit_limit(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 5)
        } else {
            self.0 & !(1 << 5)
        };
        self
    }

    #[inline]
    pub fn get_limit_maker(&self) -> bool {
        0 != self.0 & (1 << 6)
    }

    #[inline]
    pub fn set_limit_maker(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 6)
        } else {
            self.0 & !(1 << 6)
        };
        self
    }

    #[inline]
    pub fn get_non_representable(&self) -> bool {
        0 != self.0 & (1 << 15)
    }

    #[inline]
    pub fn set_non_representable(&mut self, value: bool) -> &mut Self {
        self.0 = if value {
            self.0 | (1 << 15)
        } else {
            self.0 & !(1 << 15)
        };
        self
    }
}
impl core::fmt::Debug for OrderTypes {
    #[inline]
    fn fmt(&self, fmt: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(
            fmt,
            "OrderTypes[market(0)={},limit(1)={},stop_loss(2)={},stop_loss_limit(3)={},take_profit(4)={},take_profit_limit(5)={},limit_maker(6)={},non_representable(15)={}]",
            self.get_market(),
            self.get_limit(),
            self.get_stop_loss(),
            self.get_stop_loss_limit(),
            self.get_take_profit(),
            self.get_take_profit_limit(),
            self.get_limit_maker(),
            self.get_non_representable(),
        )
    }
}
