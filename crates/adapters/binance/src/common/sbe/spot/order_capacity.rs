#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum OrderCapacity {
    Principal = 0x1_u8,
    Agency = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for OrderCapacity {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::Principal,
            0x2_u8 => Self::Agency,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<OrderCapacity> for u8 {
    #[inline]
    fn from(v: OrderCapacity) -> Self {
        match v {
            OrderCapacity::Principal => 0x1_u8,
            OrderCapacity::Agency => 0x2_u8,
            OrderCapacity::NonRepresentable => 0xfe_u8,
            OrderCapacity::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for OrderCapacity {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Principal" => Ok(Self::Principal),
            "Agency" => Ok(Self::Agency),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for OrderCapacity {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Principal => write!(f, "Principal"),
            Self::Agency => write!(f, "Agency"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
