#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum FilterType {
    MaxPosition = 0x0_u8,
    PriceFilter = 0x1_u8,
    TPlusSell = 0x2_u8,
    LotSize = 0x3_u8,
    MaxNumOrders = 0x4_u8,
    MinNotional = 0x5_u8,
    MaxNumAlgoOrders = 0x6_u8,
    ExchangeMaxNumOrders = 0x7_u8,
    ExchangeMaxNumAlgoOrders = 0x8_u8,
    IcebergParts = 0x9_u8,
    MarketLotSize = 0xa_u8,
    PercentPrice = 0xb_u8,
    MaxNumIcebergOrders = 0xc_u8,
    ExchangeMaxNumIcebergOrders = 0xd_u8,
    TrailingDelta = 0xe_u8,
    PercentPriceBySide = 0xf_u8,
    Notional = 0x10_u8,
    MaxNumOrderLists = 0x11_u8,
    ExchangeMaxNumOrderLists = 0x12_u8,
    MaxNumOrderAmends = 0x13_u8,
    MaxAsset = 0x14_u8,
    NonRepresentable = 0xfe_u8,
    #[default]
    NullVal = 0xff_u8,
}
impl From<u8> for FilterType {
    #[inline]
    fn from(v: u8) -> Self {
        match v {
            0x0_u8 => Self::MaxPosition,
            0x1_u8 => Self::PriceFilter,
            0x2_u8 => Self::TPlusSell,
            0x3_u8 => Self::LotSize,
            0x4_u8 => Self::MaxNumOrders,
            0x5_u8 => Self::MinNotional,
            0x6_u8 => Self::MaxNumAlgoOrders,
            0x7_u8 => Self::ExchangeMaxNumOrders,
            0x8_u8 => Self::ExchangeMaxNumAlgoOrders,
            0x9_u8 => Self::IcebergParts,
            0xa_u8 => Self::MarketLotSize,
            0xb_u8 => Self::PercentPrice,
            0xc_u8 => Self::MaxNumIcebergOrders,
            0xd_u8 => Self::ExchangeMaxNumIcebergOrders,
            0xe_u8 => Self::TrailingDelta,
            0xf_u8 => Self::PercentPriceBySide,
            0x10_u8 => Self::Notional,
            0x11_u8 => Self::MaxNumOrderLists,
            0x12_u8 => Self::ExchangeMaxNumOrderLists,
            0x13_u8 => Self::MaxNumOrderAmends,
            0x14_u8 => Self::MaxAsset,
            0xfe_u8 => Self::NonRepresentable,
            _ => Self::NullVal,
        }
    }
}
impl From<FilterType> for u8 {
    #[inline]
    fn from(v: FilterType) -> Self {
        match v {
            FilterType::MaxPosition => 0x0_u8,
            FilterType::PriceFilter => 0x1_u8,
            FilterType::TPlusSell => 0x2_u8,
            FilterType::LotSize => 0x3_u8,
            FilterType::MaxNumOrders => 0x4_u8,
            FilterType::MinNotional => 0x5_u8,
            FilterType::MaxNumAlgoOrders => 0x6_u8,
            FilterType::ExchangeMaxNumOrders => 0x7_u8,
            FilterType::ExchangeMaxNumAlgoOrders => 0x8_u8,
            FilterType::IcebergParts => 0x9_u8,
            FilterType::MarketLotSize => 0xa_u8,
            FilterType::PercentPrice => 0xb_u8,
            FilterType::MaxNumIcebergOrders => 0xc_u8,
            FilterType::ExchangeMaxNumIcebergOrders => 0xd_u8,
            FilterType::TrailingDelta => 0xe_u8,
            FilterType::PercentPriceBySide => 0xf_u8,
            FilterType::Notional => 0x10_u8,
            FilterType::MaxNumOrderLists => 0x11_u8,
            FilterType::ExchangeMaxNumOrderLists => 0x12_u8,
            FilterType::MaxNumOrderAmends => 0x13_u8,
            FilterType::MaxAsset => 0x14_u8,
            FilterType::NonRepresentable => 0xfe_u8,
            FilterType::NullVal => 0xff_u8,
        }
    }
}
impl core::str::FromStr for FilterType {
    type Err = ();

    #[inline]
    fn from_str(v: &str) -> core::result::Result<Self, Self::Err> {
        match v {
            "MaxPosition" => Ok(Self::MaxPosition),
            "PriceFilter" => Ok(Self::PriceFilter),
            "TPlusSell" => Ok(Self::TPlusSell),
            "LotSize" => Ok(Self::LotSize),
            "MaxNumOrders" => Ok(Self::MaxNumOrders),
            "MinNotional" => Ok(Self::MinNotional),
            "MaxNumAlgoOrders" => Ok(Self::MaxNumAlgoOrders),
            "ExchangeMaxNumOrders" => Ok(Self::ExchangeMaxNumOrders),
            "ExchangeMaxNumAlgoOrders" => Ok(Self::ExchangeMaxNumAlgoOrders),
            "IcebergParts" => Ok(Self::IcebergParts),
            "MarketLotSize" => Ok(Self::MarketLotSize),
            "PercentPrice" => Ok(Self::PercentPrice),
            "MaxNumIcebergOrders" => Ok(Self::MaxNumIcebergOrders),
            "ExchangeMaxNumIcebergOrders" => Ok(Self::ExchangeMaxNumIcebergOrders),
            "TrailingDelta" => Ok(Self::TrailingDelta),
            "PercentPriceBySide" => Ok(Self::PercentPriceBySide),
            "Notional" => Ok(Self::Notional),
            "MaxNumOrderLists" => Ok(Self::MaxNumOrderLists),
            "ExchangeMaxNumOrderLists" => Ok(Self::ExchangeMaxNumOrderLists),
            "MaxNumOrderAmends" => Ok(Self::MaxNumOrderAmends),
            "MaxAsset" => Ok(Self::MaxAsset),
            "NonRepresentable" => Ok(Self::NonRepresentable),
            _ => Ok(Self::NullVal),
        }
    }
}
impl core::fmt::Display for FilterType {
    #[inline]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::MaxPosition => write!(f, "MaxPosition"),
            Self::PriceFilter => write!(f, "PriceFilter"),
            Self::TPlusSell => write!(f, "TPlusSell"),
            Self::LotSize => write!(f, "LotSize"),
            Self::MaxNumOrders => write!(f, "MaxNumOrders"),
            Self::MinNotional => write!(f, "MinNotional"),
            Self::MaxNumAlgoOrders => write!(f, "MaxNumAlgoOrders"),
            Self::ExchangeMaxNumOrders => write!(f, "ExchangeMaxNumOrders"),
            Self::ExchangeMaxNumAlgoOrders => write!(f, "ExchangeMaxNumAlgoOrders"),
            Self::IcebergParts => write!(f, "IcebergParts"),
            Self::MarketLotSize => write!(f, "MarketLotSize"),
            Self::PercentPrice => write!(f, "PercentPrice"),
            Self::MaxNumIcebergOrders => write!(f, "MaxNumIcebergOrders"),
            Self::ExchangeMaxNumIcebergOrders => write!(f, "ExchangeMaxNumIcebergOrders"),
            Self::TrailingDelta => write!(f, "TrailingDelta"),
            Self::PercentPriceBySide => write!(f, "PercentPriceBySide"),
            Self::Notional => write!(f, "Notional"),
            Self::MaxNumOrderLists => write!(f, "MaxNumOrderLists"),
            Self::ExchangeMaxNumOrderLists => write!(f, "ExchangeMaxNumOrderLists"),
            Self::MaxNumOrderAmends => write!(f, "MaxNumOrderAmends"),
            Self::MaxAsset => write!(f, "MaxAsset"),
            Self::NonRepresentable => write!(f, "NonRepresentable"),
            Self::NullVal => write!(f, "NullVal"),
        }
    }
}
