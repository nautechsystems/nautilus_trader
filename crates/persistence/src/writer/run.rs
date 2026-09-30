// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
// -------------------------------------------------------------------------------------------------

//! Shared staged-writer session and run-state types.

use nautilus_common::enums::Environment;
use strum::{Display, EnumIter, EnumString, FromRepr};

use crate::common::storage::StorageBackend;

#[derive(Clone)]
pub struct FeatherSessionSource {
    pub storage: StorageBackend,
    pub environment: Environment,
    pub instance_id: String,
}

impl FeatherSessionSource {
    #[must_use]
    pub fn new(
        storage: StorageBackend,
        environment: Environment,
        instance_id: impl Into<String>,
    ) -> Self {
        Self {
            storage,
            environment,
            instance_id: instance_id.into(),
        }
    }
}

/// The lifecycle state of a streamed run.
#[repr(C)]
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq, FromRepr, EnumIter, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.persistence",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.persistence")
)]
pub enum RunStatus {
    InProgress,
    Completed,
    Promoted,
    Failed,
}

impl RunStatus {
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InProgress => "in_progress",
            Self::Completed => "completed",
            Self::Promoted => "promoted",
            Self::Failed => "failed",
        }
    }

    #[must_use]
    pub fn from_storage_str(value: &str) -> Self {
        match value {
            "in_progress" => Self::InProgress,
            "completed" => Self::Completed,
            "promoted" => Self::Promoted,
            "failed" => Self::Failed,
            _ => {
                log::warn!(
                    "Unknown catalog run status '{value}'; treating the run as failed for recovery"
                );
                Self::Failed
            }
        }
    }
}
