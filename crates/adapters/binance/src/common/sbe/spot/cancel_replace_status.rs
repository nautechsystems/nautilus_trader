#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum CancelReplaceStatus {
    Success = 0x0_u8,
    Failure = 0x1_u8,
    NotAttempted = 0x2_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for CancelReplaceStatus {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Success,
            0x1_u8 => Self::Failure,
            0x2_u8 => Self::NotAttempted,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<CancelReplaceStatus> for u8 {
    #[inline]
    fn from(v: CancelReplaceStatus) -> Self {
        match v {
            CancelReplaceStatus::Success => 0x0_u8,
            CancelReplaceStatus::Failure => 0x1_u8,
            CancelReplaceStatus::NotAttempted => 0x2_u8,
            CancelReplaceStatus::NonRepresentable => 0xfe_u8,
            CancelReplaceStatus::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for CancelReplaceStatus {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Success" => Ok(Self::Success),
            "Failure" => Ok(Self::Failure),
            "NotAttempted" => Ok(Self::NotAttempted),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for CancelReplaceStatus {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Success => write!(f, "Success"),
            Self::Failure => write!(f, "Failure"),
            Self::NotAttempted => write!(f, "NotAttempted"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
