// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Build script for nautilus-dydx adapter.
//!
//! This script compiles Protocol Buffer definitions for the dYdX v4 protocol using tonic-build.
//! The generated Rust code provides type-safe gRPC client interfaces for interacting with
//! dYdX v4 validator nodes.
//!
//! The script uses extern_path to reference Cosmos SDK types from the cosmos-sdk-proto crate,
//! avoiding duplication of those type definitions.

use std::{env, path::PathBuf};

fn main() {
    // Ensure rebuild when proto files change
    println!("cargo:rerun-if-changed=proto");

    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR not set"));
    let proto_dir = PathBuf::from("proto");
    let dydx_proto_dir = proto_dir.join("dydxprotocol");

    // Collect all .proto files
    let proto_files = collect_proto_files(&dydx_proto_dir).expect("Failed to collect proto files");

    if proto_files.is_empty() {
        panic!("No proto files found in {}", dydx_proto_dir.display());
    }

    // Configure tonic-build
    // - Don't generate server code (we're a client)
    // - Reference Cosmos types from cosmos-sdk-proto to avoid duplication
    // - Enable gRPC client generation
    // - Add attributes to suppress clippy warnings on generated code
    let config = tonic_build::configure();
    config
        .build_client(true)
        .build_server(false)
        .emit_rerun_if_changed(false) // We handle this manually
        .out_dir(&out_dir)
        .file_descriptor_set_path(out_dir.join("dydxprotocol_descriptor.bin"))
        .extern_path(".cosmos", "::cosmos_sdk_proto::cosmos")
        .extern_path(".cosmos_proto", "::cosmos_sdk_proto::cosmos_proto")
        .extern_path(".gogoproto", "::cosmos_sdk_proto::gogoproto")
        .type_attribute(".", "#[allow(clippy::all)]")
        .field_attribute(".", "#[allow(clippy::all)]")
        .compile_protos(&proto_files, &[proto_dir])
        .expect("Failed to compile proto files");
}

fn collect_proto_files(dir: &PathBuf) -> Result<Vec<PathBuf>, std::io::Error> {
    let mut files = Vec::new();

    if !dir.exists() {
        return Ok(files);
    }

    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_dir() {
            files.extend(collect_proto_files(&path)?);
        } else if path.extension().and_then(|s| s.to_str()) == Some("proto") {
            files.push(path);
        }
    }

    Ok(files)
}
