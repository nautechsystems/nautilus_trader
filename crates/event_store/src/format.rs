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

//! Store-level on-disk format marker.
//!
//! Written into every redb run file at creation and verified at every open before any record is
//! decoded, so a store written by a pre-codec beta build is rejected with a clear error rather than
//! surfacing as a confusing per-record decode failure.

use std::io::ErrorKind;

use redb::{
    ReadableDatabase, StorageError, TableDefinition, TableError, TransactionError, WriteTransaction,
};

use crate::error::EventStoreError;

/// On-disk store format. Bump only on a hard envelope-format break.
pub(crate) const STORE_FORMAT_VERSION: u32 = 1;

pub(crate) const BETA_REGEN_MSG: &str = "event store written by an unsupported on-disk format \
     (pre-codec beta v1.227-1.229); the format changed and these stores \
     must be regenerated";

const UNSUPPORTED_VERSION_MSG_PREFIX: &str = "unsupported event store on-disk format version ";
const FORMAT_TABLE: TableDefinition<&str, u32> = TableDefinition::new("store_format");
const FORMAT_KEY: &str = "codec";

/// Writes the current format marker.
pub(crate) fn write_store_format(txn: &WriteTransaction) -> Result<(), EventStoreError> {
    let mut table = txn.open_table(FORMAT_TABLE).map_err(map_table_err)?;
    table
        .insert(FORMAT_KEY, STORE_FORMAT_VERSION)
        .map_err(map_storage_err)?;
    Ok(())
}

/// Verifies the store carries the supported format marker.
pub(crate) fn verify_store_format<D: ReadableDatabase + ?Sized>(
    db: &D,
) -> Result<(), EventStoreError> {
    let txn = db.begin_read().map_err(map_transaction_err)?;
    let table = match txn.open_table(FORMAT_TABLE) {
        Ok(table) => table,
        Err(TableError::TableDoesNotExist(_)) => {
            return Err(EventStoreError::Corrupted(BETA_REGEN_MSG.to_string()));
        }
        Err(e) => return Err(map_table_err(e)),
    };
    let Some(value) = table.get(FORMAT_KEY).map_err(map_storage_err)? else {
        return Err(EventStoreError::Corrupted(BETA_REGEN_MSG.to_string()));
    };
    let version = value.value();

    if version == STORE_FORMAT_VERSION {
        Ok(())
    } else {
        Err(EventStoreError::Corrupted(format!(
            "{UNSUPPORTED_VERSION_MSG_PREFIX}{version}; supported version is {STORE_FORMAT_VERSION}",
        )))
    }
}

pub(crate) fn is_missing_store_format(err: &EventStoreError) -> bool {
    matches!(err, EventStoreError::Corrupted(msg) if msg == BETA_REGEN_MSG)
}

pub(crate) fn is_unsupported_store_format(err: &EventStoreError) -> bool {
    match err {
        EventStoreError::Corrupted(msg) => {
            msg == BETA_REGEN_MSG || msg.starts_with(UNSUPPORTED_VERSION_MSG_PREFIX)
        }
        _ => false,
    }
}

fn map_storage_err(err: StorageError) -> EventStoreError {
    match err {
        StorageError::Io(io_err) if is_disk_pressure(io_err.kind()) => {
            EventStoreError::Disk(io_err.to_string())
        }
        StorageError::Corrupted(msg) => EventStoreError::Corrupted(msg),
        other => EventStoreError::Backend(other.to_string()),
    }
}

fn is_disk_pressure(kind: ErrorKind) -> bool {
    matches!(
        kind,
        ErrorKind::FileTooLarge | ErrorKind::StorageFull | ErrorKind::QuotaExceeded
    )
}

fn map_table_err(err: TableError) -> EventStoreError {
    match err {
        TableError::Storage(storage) => map_storage_err(storage),
        TableError::TableDoesNotExist(_)
        | TableError::TableTypeMismatch { .. }
        | TableError::TableIsMultimap(_)
        | TableError::TableIsNotMultimap(_)
        | TableError::TypeDefinitionChanged { .. } => EventStoreError::Corrupted(err.to_string()),
        other => EventStoreError::Backend(other.to_string()),
    }
}

fn map_transaction_err(err: TransactionError) -> EventStoreError {
    match err {
        TransactionError::Storage(storage) => map_storage_err(storage),
        other => EventStoreError::Backend(other.to_string()),
    }
}
