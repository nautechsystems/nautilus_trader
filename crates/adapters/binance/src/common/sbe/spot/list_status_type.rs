#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ListStatusType {
    Response = 0x0_u8,
    ExecStarted = 0x1_u8,
    AllDone = 0x2_u8,
    Updated = 0x3_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for ListStatusType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::Response,
            0x1_u8 => Self::ExecStarted,
            0x2_u8 => Self::AllDone,
            0x3_u8 => Self::Updated,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<ListStatusType> for u8 {
    #[inline]
    fn from(v: ListStatusType) -> Self {
        match v {
            ListStatusType::Response => 0x0_u8,
            ListStatusType::ExecStarted => 0x1_u8,
            ListStatusType::AllDone => 0x2_u8,
            ListStatusType::Updated => 0x3_u8,
            ListStatusType::NonRepresentable => 0xfe_u8,
            ListStatusType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for ListStatusType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Response" => Ok(Self::Response),
            "ExecStarted" => Ok(Self::ExecStarted),
            "AllDone" => Ok(Self::AllDone),
            "Updated" => Ok(Self::Updated),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for ListStatusType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Response => write!(f, "Response"),
            Self::ExecStarted => write!(f, "ExecStarted"),
            Self::AllDone => write!(f, "AllDone"),
            Self::Updated => write!(f, "Updated"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
