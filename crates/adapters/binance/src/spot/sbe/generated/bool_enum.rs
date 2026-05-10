#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum BoolEnum {
    False = 0x0_u8,
    True = 0x1_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for BoolEnum {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::False,
            0x1_u8 => Self::True,
            _ => Self::NullVal,
        }
    }
}
impl From<BoolEnum> for u8 {
    #[inline]
    fn from(v: BoolEnum) -> Self {
        match v {
            BoolEnum::False => 0x0_u8,
            BoolEnum::True => 0x1_u8,
            BoolEnum::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for BoolEnum {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "False" => Ok(Self::False),
            "True" => Ok(Self::True),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for BoolEnum {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::False => write!(f, "False"),
            Self::True => write!(f, "True"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
