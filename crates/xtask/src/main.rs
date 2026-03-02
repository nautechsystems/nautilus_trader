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

//! Developer tooling for the NautilusTrader workspace.
//!
//! # Commands
//!
//! - `error-index` — Generate `docs/error-codes.md` from `impl_error_codes!` macro invocations.
//! - `error-index --check` — Verify the committed file matches the generated output (for CI).

use std::{
    collections::{BTreeMap, HashMap},
    env, fmt,
    fs::{self},
    path::{Path, PathBuf},
    process,
};

use regex::Regex;
use walkdir::WalkDir;

/// A single error code entry extracted from source.
#[derive(Debug, Clone)]
struct ErrorEntry {
    code: String,
    type_name: String,
    variant: String,
    error_message: String,
    doc_comment: String,
    file: PathBuf,
}

/// Section header mapping from code prefix to human-readable group name.
fn section_name(code: &str) -> &str {
    // NT-02xx, NT-03xx, etc.
    match &code[..5] {
        "NT-02" => "Order Book",
        "NT-03" => "Orders",
        "NT-04" => "Data Parsing",
        _ => "Other",
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(String::as_str);
    let check_mode = args.iter().any(|a| a == "--check");

    match command {
        Some("error-index") => run_error_index(check_mode),
        _ => {
            eprintln!("Usage: nautilus-xtask <command>");
            eprintln!();
            eprintln!("Commands:");
            eprintln!("  error-index          Generate docs/error-codes.md");
            eprintln!("  error-index --check   Verify docs/error-codes.md is up to date");
            process::exit(1);
        }
    }
}

fn run_error_index(check_mode: bool) {
    let workspace_root = find_workspace_root();
    let crates_dir = workspace_root.join("crates");
    let output_path = workspace_root.join("docs").join("error-codes.md");

    let mut entries: Vec<ErrorEntry> = Vec::new();

    // Walk all .rs files under crates/, excluding xtask itself
    for entry in WalkDir::new(&crates_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
            // Skip target dirs, hidden dirs, and the xtask crate
            !name.starts_with('.') && name != "target" && name != "xtask"
        })
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file() && e.path().extension().is_some_and(|ext| ext == "rs"))
    {
        let path = entry.path();
        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => continue,
        };

        // Extract from impl_error_codes! macro invocations
        extract_macro_entries(&content, path, &mut entries);

        // Extract from manual `impl ... ErrorCode for ...` blocks
        extract_manual_impl_entries(&content, path, &mut entries);
    }

    // Check for duplicate codes
    let mut seen: HashMap<String, &ErrorEntry> = HashMap::new();
    let mut duplicates = Vec::new();
    for entry in &entries {
        if let Some(prev) = seen.get(&entry.code) {
            duplicates.push(format!(
                "  Duplicate code {}: {}::{} (in {}) and {}::{} (in {})",
                entry.code,
                prev.type_name,
                prev.variant,
                prev.file.display(),
                entry.type_name,
                entry.variant,
                entry.file.display(),
            ));
        } else {
            seen.insert(entry.code.clone(), entry);
        }
    }

    if !duplicates.is_empty() {
        eprintln!("Error: duplicate error codes found:");
        for d in &duplicates {
            eprintln!("{d}");
        }
        process::exit(1);
    }

    // Sort entries by code
    entries.sort_by(|a, b| a.code.cmp(&b.code));

    let markdown = generate_markdown(&entries);

    if check_mode {
        let existing = fs::read_to_string(&output_path).unwrap_or_default();
        if existing == markdown {
            println!("docs/error-codes.md is up to date.");
        } else {
            eprintln!(
                "docs/error-codes.md is out of date. Run `cargo run -p nautilus-xtask -- error-index` to regenerate."
            );
            process::exit(1);
        }
    } else {
        // Ensure docs/ directory exists
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent).expect("Failed to create docs directory");
        }
        fs::write(&output_path, &markdown).expect("Failed to write docs/error-codes.md");
        println!(
            "Generated {} with {} error codes.",
            output_path.display(),
            entries.len()
        );
    }
}

/// Extract error entries from `impl_error_codes!` macro invocations.
fn extract_macro_entries(content: &str, path: &Path, entries: &mut Vec<ErrorEntry>) {
    // Match: impl_error_codes! { TypeName { ... } }
    let macro_re =
        Regex::new(r"(?s)impl_error_codes!\s*\{\s*(\w+)\s*\{(.*?)\}\s*\}").expect("valid regex");

    // Match each variant => code line, with optional preceding doc comments
    let variant_re = Regex::new(
        r#"(?m)(?P<docs>(?:\s*///[^\n]*\n)*)?\s*(?P<variant>\w+)\s*(?:\([^)]*\))?\s*=>\s*"(?P<code>NT-\d+)""#,
    )
    .expect("valid regex");

    // For extracting #[error("...")] messages from the enum definition
    let error_attr_re =
        Regex::new(r#"(?m)#\[error\("([^"]+)"\)\]\s*\n\s*(\w+)"#).expect("valid regex");

    // Build a map of TypeName::Variant -> error message from #[error(...)] attributes
    let mut error_messages: HashMap<String, String> = HashMap::new();
    for cap in error_attr_re.captures_iter(content) {
        let msg = cap.get(1).expect("capture group").as_str();
        let variant = cap.get(2).expect("capture group").as_str();
        error_messages.insert(variant.to_string(), msg.to_string());
    }

    for macro_cap in macro_re.captures_iter(content) {
        let type_name = macro_cap.get(1).expect("capture group").as_str();
        let body = macro_cap.get(2).expect("capture group").as_str();

        for var_cap in variant_re.captures_iter(body) {
            let variant = var_cap.name("variant").expect("capture group").as_str();
            let code = var_cap.name("code").expect("capture group").as_str();
            let docs_raw = var_cap.name("docs").map_or("", |m| m.as_str());

            // Clean doc comment lines: strip leading `///` and whitespace
            let doc_comment: String = docs_raw
                .lines()
                .map(|line| line.trim().trim_start_matches("///").trim())
                .filter(|line| !line.is_empty())
                .collect::<Vec<_>>()
                .join(" ");

            let error_message = error_messages.get(variant).cloned().unwrap_or_default();

            entries.push(ErrorEntry {
                code: code.to_string(),
                type_name: type_name.to_string(),
                variant: variant.to_string(),
                error_message,
                doc_comment,
                file: path.to_path_buf(),
            });
        }
    }
}

/// Extract error entries from manual `impl ErrorCode for Type` blocks.
fn extract_manual_impl_entries(content: &str, path: &Path, entries: &mut Vec<ErrorEntry>) {
    // Match: impl [path::]ErrorCode for TypeName { fn code(...) { "NT-XXXX" } }
    let manual_re = Regex::new(
        r#"(?s)impl\s+(?:\w+::)*ErrorCode\s+for\s+(\w+)\s*\{[^}]*fn\s+code\s*\([^)]*\)\s*->\s*&'static\s+str\s*\{\s*"(NT-\d+)"\s*\}"#,
    )
    .expect("valid regex");

    // For extracting the #[error("...")] from struct-level thiserror
    let struct_error_re = Regex::new(r#"(?s)#\[error\("([^"]+)"\)\]\s*\n\s*pub\s+struct\s+(\w+)"#)
        .expect("valid regex");

    let mut struct_messages: HashMap<String, String> = HashMap::new();
    for cap in struct_error_re.captures_iter(content) {
        let msg = cap.get(1).expect("capture group").as_str();
        let name = cap.get(2).expect("capture group").as_str();
        struct_messages.insert(name.to_string(), msg.to_string());
    }

    for cap in manual_re.captures_iter(content) {
        let type_name = cap.get(1).expect("capture group").as_str();
        let code = cap.get(2).expect("capture group").as_str();

        let error_message = struct_messages.get(type_name).cloned().unwrap_or_default();

        entries.push(ErrorEntry {
            code: code.to_string(),
            type_name: type_name.to_string(),
            variant: String::new(), // struct errors have no variant
            error_message,
            doc_comment: String::new(),
            file: path.to_path_buf(),
        });
    }
}

fn generate_markdown(entries: &[ErrorEntry]) -> String {
    let mut out = String::new();
    out.push_str("# NautilusTrader Error Code Index\n\n");
    out.push_str("> Auto-generated by `cargo run -p nautilus-xtask -- error-index`. Do not edit manually.\n\n");

    // Group entries by their NT-XX prefix
    let mut groups: BTreeMap<String, Vec<&ErrorEntry>> = BTreeMap::new();
    for entry in entries {
        let prefix = if entry.code.len() >= 5 {
            format!("{}xx", &entry.code[..5])
        } else {
            "other".to_string()
        };
        groups.entry(prefix).or_default().push(entry);
    }

    for (prefix, group_entries) in &groups {
        let name = section_name(&group_entries[0].code);
        out.push_str(&fmt::format(format_args!("## {prefix} — {name}\n\n")));
        out.push_str("| Code | Error Type | Variant | Message | Description |\n");
        out.push_str("|------|-----------|---------|---------|-------------|\n");

        for entry in group_entries {
            let variant_display = if entry.variant.is_empty() {
                "—".to_string()
            } else {
                format!("`{}`", entry.variant)
            };

            let desc = if entry.doc_comment.is_empty() {
                "—".to_string()
            } else {
                entry.doc_comment.clone()
            };

            let msg = if entry.error_message.is_empty() {
                "—".to_string()
            } else {
                entry.error_message.clone()
            };

            out.push_str(&fmt::format(format_args!(
                "| `{}` | `{}` | {} | {} | {} |\n",
                entry.code, entry.type_name, variant_display, msg, desc,
            )));
        }

        out.push('\n');
    }

    out
}

/// Find the workspace root by looking for the top-level Cargo.toml with \[workspace\].
fn find_workspace_root() -> PathBuf {
    let mut dir = env::current_dir().expect("Failed to get current directory");
    loop {
        let candidate = dir.join("Cargo.toml");
        if candidate.exists()
            && let Ok(content) = fs::read_to_string(&candidate)
            && content.contains("[workspace]")
        {
            return dir;
        }
        if !dir.pop() {
            eprintln!("Error: could not find workspace root (no Cargo.toml with [workspace])");
            process::exit(1);
        }
    }
}
