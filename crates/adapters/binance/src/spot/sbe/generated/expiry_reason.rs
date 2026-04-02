#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum ExpiryReason {
    Rejected = 0x1_u8,
    ExchangeCanceled = 0x2_u8,
    OcoTrigger = 0x3_u8,
    OtoPhaseOneExpired = 0x4_u8,
    UnfilledIocQuantityExpired = 0x5_u8,
    UnfilledFokOrderExpired = 0x6_u8,
    InsufficientLiquidity = 0x7_u8,
    ExecutionRulePriceRangeExceeded = 0x8_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for ExpiryReason {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x1_u8 => Self::Rejected,
            0x2_u8 => Self::ExchangeCanceled,
            0x3_u8 => Self::OcoTrigger,
            0x4_u8 => Self::OtoPhaseOneExpired,
            0x5_u8 => Self::UnfilledIocQuantityExpired,
            0x6_u8 => Self::UnfilledFokOrderExpired,
            0x7_u8 => Self::InsufficientLiquidity,
            0x8_u8 => Self::ExecutionRulePriceRangeExceeded,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<ExpiryReason> for u8 {
    #[inline]
    fn from(v: ExpiryReason) -> Self {
        match v {
            ExpiryReason::Rejected => 0x1_u8,
            ExpiryReason::ExchangeCanceled => 0x2_u8,
            ExpiryReason::OcoTrigger => 0x3_u8,
            ExpiryReason::OtoPhaseOneExpired => 0x4_u8,
            ExpiryReason::UnfilledIocQuantityExpired => 0x5_u8,
            ExpiryReason::UnfilledFokOrderExpired => 0x6_u8,
            ExpiryReason::InsufficientLiquidity => 0x7_u8,
            ExpiryReason::ExecutionRulePriceRangeExceeded => 0x8_u8,
            ExpiryReason::NonRepresentable => 0xfe_u8,
            ExpiryReason::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for ExpiryReason {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "Rejected" => Ok(Self::Rejected),
            "ExchangeCanceled" => Ok(Self::ExchangeCanceled),
            "OcoTrigger" => Ok(Self::OcoTrigger),
            "OtoPhaseOneExpired" => Ok(Self::OtoPhaseOneExpired),
            "UnfilledIocQuantityExpired" => Ok(Self::UnfilledIocQuantityExpired),
            "UnfilledFokOrderExpired" => Ok(Self::UnfilledFokOrderExpired),
            "InsufficientLiquidity" => Ok(Self::InsufficientLiquidity),
            "ExecutionRulePriceRangeExceeded" => Ok(Self::ExecutionRulePriceRangeExceeded),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for ExpiryReason {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::Rejected => write!(f, "Rejected"),
            Self::ExchangeCanceled => write!(f, "ExchangeCanceled"),
            Self::OcoTrigger => write!(f, "OcoTrigger"),
            Self::OtoPhaseOneExpired => write!(f, "OtoPhaseOneExpired"),
            Self::UnfilledIocQuantityExpired => write!(f, "UnfilledIocQuantityExpired"),
            Self::UnfilledFokOrderExpired => write!(f, "UnfilledFokOrderExpired"),
            Self::InsufficientLiquidity => write!(f, "InsufficientLiquidity"),
            Self::ExecutionRulePriceRangeExceeded => write!(f, "ExecutionRulePriceRangeExceeded"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
