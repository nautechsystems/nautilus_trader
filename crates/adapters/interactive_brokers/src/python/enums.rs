// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Python bindings for Interactive Brokers adapter enums.

use pyo3::prelude::*;

use crate::{
    common::enums::{
        IbAccountSummaryEvent, IbAccountUpdateEvent, IbAccountUpdateMultiEvent, IbAction,
        IbArticleType, IbAuctionStrategy, IbAuctionType, IbBondIdentifierKind,
        IbBuilderTimeInForce, IbCancelOrderEvent, IbComboLegOpenClose, IbConditionConjunction,
        IbConditionKind, IbExecutionsEvent, IbExerciseAction, IbExerciseOptionsEvent,
        IbFundAssetType, IbFundDistributionPolicyIndicator, IbHistoricalBarSize,
        IbHistoricalBarUpdateEvent, IbHistoricalTickType, IbHistoricalWhatToShow, IbLegAction,
        IbLiquidity, IbMarketDepthEvent, IbOcaType, IbOptionRight, IbOrderOpenClose, IbOrderOrigin,
        IbOrderStatus, IbOrderType, IbOrderUpdateEvent, IbOrdersEvent, IbPlaceOrderEvent,
        IbPositionUpdateEvent, IbPositionUpdateMultiEvent, IbRealtimeBarSize, IbRealtimeWhatToShow,
        IbReferencePriceType, IbRiskAversion, IbRule80A, IbSecurityType, IbShortSaleSlot,
        IbTickEvent, IbTickType, IbTimeInForce, IbTradingHours, IbTriggerMethod,
        IbTwapStrategyType, IbVolatilityType,
    },
    error::{ErrorCategory, InteractiveBrokersErrorKind},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbAction {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbOrderStatus {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbOrderType {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbTimeInForce {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbBuilderTimeInForce {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbSecurityType {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbOptionRight {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbHistoricalTickType {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbTradingHours {
    #[pyo3(name = "use_rth")]
    fn py_use_rth(&self) -> bool {
        self.use_rth()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbHistoricalBarSize {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbHistoricalWhatToShow {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbRealtimeBarSize {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> String {
        self.to_string()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbRealtimeWhatToShow {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbConditionKind {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbConditionConjunction {
    #[pyo3(name = "as_str")]
    fn py_as_str(&self) -> &'static str {
        self.as_str()
    }

    #[pyo3(name = "is_conjunction")]
    fn py_is_conjunction(&self) -> bool {
        self.is_conjunction()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbComboLegOpenClose {
    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbTriggerMethod {
    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbOcaType {
    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl IbLiquidity {
    #[pyo3(name = "as_i32")]
    fn py_as_i32(&self) -> i32 {
        self.as_i32()
    }
}

macro_rules! py_i32_enum {
    ($ty:ty) => {
        #[pymethods]
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        impl $ty {
            #[pyo3(name = "as_i32")]
            fn py_as_i32(&self) -> i32 {
                self.as_i32()
            }
        }
    };
}

macro_rules! py_str_enum {
    ($ty:ty) => {
        #[pymethods]
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        impl $ty {
            #[pyo3(name = "as_str")]
            fn py_as_str(&self) -> String {
                self.to_string()
            }
        }
    };
}

macro_rules! py_marker_enum {
    ($ty:ty) => {
        #[pymethods]
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        impl $ty {
            #[pyo3(name = "as_str")]
            fn py_as_str(&self) -> String {
                format!("{self:?}")
            }
        }
    };
}

py_i32_enum!(IbTickType);

py_i32_enum!(IbOrderOrigin);
py_i32_enum!(IbShortSaleSlot);
py_i32_enum!(IbVolatilityType);
py_i32_enum!(IbReferencePriceType);
py_i32_enum!(IbAuctionStrategy);
py_i32_enum!(IbExerciseAction);
py_i32_enum!(IbArticleType);
py_i32_enum!(IbAuctionType);

py_str_enum!(IbRule80A);
py_str_enum!(IbOrderOpenClose);
py_str_enum!(IbTwapStrategyType);
py_str_enum!(IbRiskAversion);
py_str_enum!(IbLegAction);
py_str_enum!(IbFundDistributionPolicyIndicator);
py_str_enum!(IbFundAssetType);
py_str_enum!(IbBondIdentifierKind);

py_marker_enum!(IbPlaceOrderEvent);
py_marker_enum!(IbOrderUpdateEvent);
py_marker_enum!(IbCancelOrderEvent);
py_marker_enum!(IbOrdersEvent);
py_marker_enum!(IbExecutionsEvent);
py_marker_enum!(IbExerciseOptionsEvent);
py_marker_enum!(IbHistoricalBarUpdateEvent);
py_marker_enum!(IbMarketDepthEvent);
py_marker_enum!(IbTickEvent);
py_marker_enum!(IbAccountSummaryEvent);
py_marker_enum!(IbPositionUpdateEvent);
py_marker_enum!(IbPositionUpdateMultiEvent);
py_marker_enum!(IbAccountUpdateEvent);
py_marker_enum!(IbAccountUpdateMultiEvent);
py_marker_enum!(ErrorCategory);
py_marker_enum!(InteractiveBrokersErrorKind);
