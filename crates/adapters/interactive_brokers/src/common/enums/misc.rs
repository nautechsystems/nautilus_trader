// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

use std::fmt::Display;

macro_rules! define_ib_i32_enum {
    (
        $(#[$meta:meta])*
        pub enum $name:ident {
            $(
                $(#[$variant_meta:meta])*
                $variant:ident = $value:expr
            ),+ $(,)?
        }
    ) => {
        $(#[$meta])*
        #[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
        #[cfg_attr(
            feature = "python",
            pyo3::pyclass(
                module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
                from_py_object
            )
        )]
        pub enum $name {
            $($(#[$variant_meta])* $variant),+
        }

        impl $name {
            #[must_use]
            pub const fn as_i32(self) -> i32 {
                match self {
                    $(Self::$variant => $value),+
                }
            }
        }

        impl From<i32> for $name {
            fn from(value: i32) -> Self {
                match value {
                    $($value => Self::$variant,)+
                    _ => Self::default(),
                }
            }
        }

        impl Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.as_i32())
            }
        }
    };
}

define_ib_i32_enum! {
    /// Interactive Brokers order origin values.
    pub enum IbOrderOrigin {
        #[default]
        Customer = 0,
        Firm = 1,
    }
}

impl IbOrderOrigin {
    #[must_use]
    pub fn ibapi_order_origin(self) -> ibapi::orders::OrderOrigin {
        ibapi::orders::OrderOrigin::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers institutional short-sale slot values.
    pub enum IbShortSaleSlot {
        #[default]
        None = 0,
        Broker = 1,
        ThirdParty = 2,
    }
}

impl IbShortSaleSlot {
    #[must_use]
    pub fn ibapi_short_sale_slot(self) -> ibapi::orders::ShortSaleSlot {
        ibapi::orders::ShortSaleSlot::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers volatility type values.
    pub enum IbVolatilityType {
        #[default]
        Daily = 1,
        Annual = 2,
    }
}

impl IbVolatilityType {
    #[must_use]
    pub fn ibapi_volatility_type(self) -> ibapi::orders::VolatilityType {
        ibapi::orders::VolatilityType::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers VOL order reference price type values.
    pub enum IbReferencePriceType {
        #[default]
        AverageOfNbbo = 1,
        Nbbo = 2,
    }
}

impl IbReferencePriceType {
    #[must_use]
    pub fn ibapi_reference_price_type(self) -> ibapi::orders::ReferencePriceType {
        ibapi::orders::ReferencePriceType::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers BOX auction strategy values.
    pub enum IbAuctionStrategy {
        #[default]
        Match = 1,
        Improvement = 2,
        Transparent = 3,
    }
}

impl IbAuctionStrategy {
    #[must_use]
    pub fn ibapi_auction_strategy(self) -> ibapi::orders::AuctionStrategy {
        ibapi::orders::AuctionStrategy::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers option exercise action values.
    pub enum IbExerciseAction {
        #[default]
        Exercise = 1,
        Lapse = 2,
    }
}

impl IbExerciseAction {
    #[must_use]
    pub const fn ibapi_exercise_action(self) -> ibapi::orders::ExerciseAction {
        match self {
            Self::Exercise => ibapi::orders::ExerciseAction::Exercise,
            Self::Lapse => ibapi::orders::ExerciseAction::Lapse,
        }
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers news article type values.
    pub enum IbArticleType {
        #[default]
        Text = 0,
        Binary = 1,
    }
}

impl IbArticleType {
    #[must_use]
    pub fn ibapi_article_type(self) -> ibapi::news::ArticleType {
        ibapi::news::ArticleType::from(self.as_i32())
    }
}

define_ib_i32_enum! {
    /// Interactive Brokers builder auction type values.
    pub enum IbAuctionType {
        #[default]
        Opening = 1,
        Closing = 2,
        Volatility = 4,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRule80A {
    Individual,
    Agency,
    AgentOtherMember,
    IndividualPtia,
    AgencyPtia,
    AgentOtherMemberPtia,
    IndividualPt,
    AgencyPt,
    AgentOtherMemberPt,
}

impl IbRule80A {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Individual => "I",
            Self::Agency => "A",
            Self::AgentOtherMember => "W",
            Self::IndividualPtia => "J",
            Self::AgencyPtia => "U",
            Self::AgentOtherMemberPtia => "M",
            Self::IndividualPt => "K",
            Self::AgencyPt => "Y",
            Self::AgentOtherMemberPt => "N",
        }
    }
}

impl Display for IbRule80A {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbOrderOpenClose {
    Open,
    Close,
}

impl IbOrderOpenClose {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "O",
            Self::Close => "C",
        }
    }
}

impl Display for IbOrderOpenClose {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbTwapStrategyType {
    Marketable,
    MatchingMidpoint,
    MatchingSameSide,
    MatchingLast,
}

impl IbTwapStrategyType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Marketable => "Marketable",
            Self::MatchingMidpoint => "Matching Midpoint",
            Self::MatchingSameSide => "Matching Same Side",
            Self::MatchingLast => "Matching Last",
        }
    }
}

impl Display for IbTwapStrategyType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbRiskAversion {
    GetDone,
    Aggressive,
    Neutral,
    Passive,
}

impl IbRiskAversion {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::GetDone => "Get Done",
            Self::Aggressive => "Aggressive",
            Self::Neutral => "Neutral",
            Self::Passive => "Passive",
        }
    }
}

impl Display for IbRiskAversion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbLegAction {
    Buy,
    Sell,
}

impl IbLegAction {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Buy => "BUY",
            Self::Sell => "SELL",
        }
    }
}

impl Display for IbLegAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbFundDistributionPolicyIndicator {
    None,
    AccumulationFund,
    IncomeFund,
}

impl IbFundDistributionPolicyIndicator {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::AccumulationFund => "N",
            Self::IncomeFund => "Y",
        }
    }
}

impl Display for IbFundDistributionPolicyIndicator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbFundAssetType {
    None,
    Others,
    MoneyMarket,
    FixedIncome,
    MultiAsset,
    Equity,
    Sector,
    Guaranteed,
    Alternative,
}

impl IbFundAssetType {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::None => "",
            Self::Others => "000",
            Self::MoneyMarket => "001",
            Self::FixedIncome => "002",
            Self::MultiAsset => "003",
            Self::Equity => "004",
            Self::Sector => "005",
            Self::Guaranteed => "006",
            Self::Alternative => "007",
        }
    }
}

impl Display for IbFundAssetType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Bond identifier discriminator for the rust-ibapi payload enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.interactive_brokers",
        from_py_object
    )
)]
pub enum IbBondIdentifierKind {
    Cusip,
    Isin,
}

impl IbBondIdentifierKind {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cusip => "CUSIP",
            Self::Isin => "ISIN",
        }
    }
}

impl Display for IbBondIdentifierKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}
