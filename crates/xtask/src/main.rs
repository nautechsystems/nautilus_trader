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
//! - `error-index` — Generate `docs/error-codes.md` from `#[error("[NT-XXXX] ...")]` attributes.
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
    crate_dir: String,
}

/// A thiserror variant missing an error code.
#[derive(Debug)]
struct MissingCode {
    type_name: String,
    variant: String,
    file: PathBuf,
}

/// Info about a single variant in an error enum.
struct VariantInfo {
    variant: String,
    code: Option<String>,
    message: String,
    doc_comment: String,
}

/// Canonical mapping from crate directory name to (indicator, human-readable name).
///
/// This is the single source of truth for crate abbreviations. The xtask validates
/// that error codes use the correct indicator for the crate they are defined in,
/// and this table is rendered into the generated docs.
const CRATE_ABBREVIATIONS: &[(&str, &str, &str)] = &[
    ("adapters", "AD", "Adapters"),
    ("analysis", "AN", "Analysis"),
    ("backtest", "BT", "Backtest"),
    ("cli", "CL", "CLI"),
    ("common", "CM", "Common"),
    ("core", "CR", "Core"),
    ("cryptography", "CY", "Cryptography"),
    ("data", "DA", "Data"),
    ("databento", "DB", "Databento"),
    ("execution", "EX", "Execution"),
    ("indicators", "IN", "Indicators"),
    ("infrastructure", "IF", "Infrastructure"),
    ("live", "LV", "Live"),
    ("model", "MD", "Model"),
    ("network", "NW", "Network"),
    ("persistence", "PS", "Persistence"),
    ("portfolio", "PF", "Portfolio"),
    ("risk", "RK", "Risk"),
    ("serialization", "SR", "Serialization"),
    ("system", "SY", "System"),
    ("tardis", "TD", "Tardis"),
    ("trading", "TR", "Trading"),
];

/// Look up the human-readable crate name from a two-letter indicator.
fn crate_name(indicator: &str) -> &str {
    CRATE_ABBREVIATIONS
        .iter()
        .find(|(_, ind, _)| *ind == indicator)
        .map_or("Other", |(_, _, name)| *name)
}

/// Look up the expected two-letter indicator for a crate directory name.
fn expected_indicator(crate_dir: &str) -> Option<&str> {
    CRATE_ABBREVIATIONS
        .iter()
        .find(|(dir, _, _)| *dir == crate_dir)
        .map(|(_, ind, _)| *ind)
}

/// Section header mapping from code prefix to human-readable group name.
/// Code format: NT-XX-YYYYY where XX is the crate indicator and YYY is the domain prefix.
fn section_name(code: &str) -> &str {
    // Extract the 5-digit number portion: NT-XX-YYYYY -> first 3 digits = domain
    let digits = &code[6..11]; // "YYYYY"
    match &digits[..3] {
        "002" => "Order Book",
        "003" => "Orders",
        "004" => "Data Parsing",
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
    let mut missing: Vec<MissingCode> = Vec::new();

    // Walk all .rs files under crates/, excluding xtask itself
    for entry in WalkDir::new(&crates_dir)
        .into_iter()
        .filter_entry(|e| {
            let name = e.file_name().to_string_lossy();
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

        // Extract crate directory name from path: crates/<crate_dir>/...
        let crate_dir = path
            .strip_prefix(&crates_dir)
            .ok()
            .and_then(|rel| rel.components().next())
            .map(|c| c.as_os_str().to_string_lossy().to_string())
            .unwrap_or_default();

        extract_error_entries(&content, path, &crate_dir, &mut entries, &mut missing);
    }

    // Report variants missing error codes
    if !missing.is_empty() {
        eprintln!("Error: thiserror variants missing error codes (in types that use error codes):");
        for m in &missing {
            eprintln!("  {}::{} (in {})", m.type_name, m.variant, m.file.display());
        }
        process::exit(1);
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

    // Validate crate indicators match the crate where the error is defined
    let mut mismatches = Vec::new();
    for entry in &entries {
        let code_indicator = &entry.code[3..5]; // "XX" from "NT-XX-YYYYY"
        if let Some(expected) = expected_indicator(&entry.crate_dir) {
            if code_indicator != expected {
                mismatches.push(format!(
                    "  {}: {}::{} uses indicator '{}' but is in crate '{}' (expected '{}')",
                    entry.code,
                    entry.type_name,
                    entry.variant,
                    code_indicator,
                    entry.crate_dir,
                    expected,
                ));
            }
        } else {
            mismatches.push(format!(
                "  {}: {}::{} is in unknown crate '{}' — add it to CRATE_ABBREVIATIONS",
                entry.code, entry.type_name, entry.variant, entry.crate_dir,
            ));
        }
    }

    if !mismatches.is_empty() {
        eprintln!("Error: crate indicator mismatches in error codes:");
        for m in &mismatches {
            eprintln!("{m}");
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

/// Extract the doc comment block (consecutive `///` lines) immediately preceding `byte_offset`.
///
/// Walks backward from the position just before the `#[error(...)]` attribute,
/// collecting `///` comment lines until a non-doc-comment line is hit.
fn extract_doc_comment(content: &str, byte_offset: usize) -> String {
    let before = &content[..byte_offset];
    let lines: Vec<&str> = before.lines().collect();

    let mut doc_lines = Vec::new();
    for line in lines.iter().rev() {
        let trimmed = line.trim();
        if let Some(stripped) = trimmed.strip_prefix("///") {
            doc_lines.push(stripped.trim());
        } else if trimmed.is_empty() {
            // Allow blank lines between doc comments (though unusual)
            continue;
        } else {
            break;
        }
    }

    doc_lines.reverse();
    doc_lines.join(" ")
}

/// Extract error entries from `#[error("[NT-XXXX] ...")]` attributes on thiserror types.
///
/// Also extracts `///` doc comments preceding the `#[error(...)]` attribute.
/// Only flags missing codes for types where at least one variant has an error code,
/// so types that haven't adopted error codes yet are not flagged.
fn extract_error_entries(
    content: &str,
    path: &Path,
    crate_dir: &str,
    entries: &mut Vec<ErrorEntry>,
    missing: &mut Vec<MissingCode>,
) {
    // Match #[error("...")] followed by a variant name (enum variant or struct name).
    // Allows optional whitespace/newlines between #[error( and the string literal,
    // and between the string literal and )], to handle reformatted multiline attributes.
    let error_attr_re = Regex::new(
        r#"(?m)#\[error\(\s*"(?P<msg>[^"]+)"\s*\)\]\s*\n\s*(?:pub\s+)?(?:struct\s+)?(?P<name>\w+)"#,
    )
    .expect("valid regex");

    // Match the NT code at the start of an error message: [NT-XX-XXXXX]
    let code_re =
        Regex::new(r"^\[(?P<code>NT-[A-Z]{2}-\d{5})\]\s*(?P<rest>.*)").expect("valid regex");

    // Find enum/struct type declarations
    let type_decl_re =
        Regex::new(r"(?m)pub\s+(?:enum|struct)\s+(?P<name>\w+)").expect("valid regex");

    let mut type_ranges: Vec<(usize, String)> = Vec::new();
    for cap in type_decl_re.captures_iter(content) {
        let name = cap.name("name").expect("capture group").as_str();
        let offset = cap.get(0).expect("capture group").start();
        type_ranges.push((offset, name.to_string()));
    }

    // Collect all variants per type
    let mut type_variants: HashMap<String, Vec<VariantInfo>> = HashMap::new();

    for cap in error_attr_re.captures_iter(content) {
        let msg = cap.name("msg").expect("capture group").as_str();
        let name = cap.name("name").expect("capture group").as_str();
        let offset = cap.get(0).expect("capture group").start();

        // Extract doc comment preceding this #[error(...)] attribute
        let doc_comment = extract_doc_comment(content, offset);

        // Find the parent type
        let type_name = type_ranges
            .iter()
            .rev()
            .find(|(type_offset, _)| *type_offset <= offset)
            .map_or_else(|| name.to_string(), |(_, n)| n.clone());

        // For struct-level errors, the name IS the type name
        let is_struct = cap
            .get(0)
            .expect("capture group")
            .as_str()
            .contains("struct");
        let (display_type, variant) = if is_struct {
            (name.to_string(), String::new())
        } else {
            (type_name, name.to_string())
        };

        let (code, rest) = if let Some(code_cap) = code_re.captures(msg) {
            let code = code_cap
                .name("code")
                .expect("capture group")
                .as_str()
                .to_string();
            let rest = code_cap
                .name("rest")
                .expect("capture group")
                .as_str()
                .to_string();
            (Some(code), rest)
        } else {
            (None, msg.to_string())
        };

        type_variants
            .entry(display_type)
            .or_default()
            .push(VariantInfo {
                variant,
                code,
                message: rest,
                doc_comment,
            });
    }

    // Process each type: only flag missing codes if at least one variant has a code
    for (type_name, variants) in &type_variants {
        let has_any_code = variants.iter().any(|v| v.code.is_some());

        for v in variants {
            if let Some(code) = &v.code {
                entries.push(ErrorEntry {
                    code: code.clone(),
                    type_name: type_name.clone(),
                    variant: v.variant.clone(),
                    error_message: v.message.clone(),
                    doc_comment: v.doc_comment.clone(),
                    file: path.to_path_buf(),
                    crate_dir: crate_dir.to_string(),
                });
            } else if has_any_code {
                missing.push(MissingCode {
                    type_name: type_name.clone(),
                    variant: if v.variant.is_empty() {
                        type_name.clone()
                    } else {
                        v.variant.clone()
                    },
                    file: path.to_path_buf(),
                });
            }
        }
    }
}

fn generate_markdown(entries: &[ErrorEntry]) -> String {
    let mut out = String::new();
    out.push_str("# NautilusTrader Error Code Index\n\n");
    out.push_str(
        "> Auto-generated by `cargo run -p nautilus-xtask -- error-index`. Do not edit manually.\n\n",
    );

    // Crate abbreviation legend
    out.push_str("## Crate Abbreviations\n\n");
    out.push_str("Error codes follow the format `NT-XX-YYYYY` where `XX` is the crate indicator and `YYYYY` is a 5-digit code.\n\n");
    out.push_str("| Abbreviation | Crate |\n");
    out.push_str("|:---:|---|\n");
    for (dir, ind, name) in CRATE_ABBREVIATIONS {
        out.push_str(&fmt::format(format_args!(
            "| `{ind}` | {name} (`{dir}`) |\n"
        )));
    }
    out.push('\n');

    // Group entries by crate indicator + domain prefix: NT-XX-YYYxx
    let mut groups: BTreeMap<String, Vec<&ErrorEntry>> = BTreeMap::new();
    for entry in entries {
        if entry.code.len() >= 11 {
            // NT-XX-YYYYY -> group by NT-XX-YYYxx (crate + first 3 domain digits)
            let prefix = format!("{}xx", &entry.code[..9]);
            groups.entry(prefix).or_default().push(entry);
        } else {
            groups.entry("other".to_string()).or_default().push(entry);
        }
    }

    for (prefix, group_entries) in &groups {
        let code = &group_entries[0].code;
        let crate_ind = &code[3..5]; // "XX" from "NT-XX-YYYYY"
        let domain = section_name(code);
        let crate_n = crate_name(crate_ind);
        out.push_str(&fmt::format(format_args!(
            "## {prefix} — {crate_n} / {domain}\n\n"
        )));
        out.push_str("| Code | Error Type | Variant | Message | Description |\n");
        out.push_str("|------|-----------|---------|---------|-------------|\n");

        for entry in group_entries {
            let variant_display = if entry.variant.is_empty() {
                "—".to_string()
            } else {
                format!("`{}`", entry.variant)
            };

            let msg = if entry.error_message.is_empty() {
                "—".to_string()
            } else {
                entry.error_message.clone()
            };

            let desc = if entry.doc_comment.is_empty() {
                "—".to_string()
            } else {
                entry.doc_comment.clone()
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
