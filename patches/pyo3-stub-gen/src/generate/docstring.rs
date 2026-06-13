use std::fmt;

/// Normalize a docstring by trimming outer whitespace and dedenting.
///
/// Implements Python's inspect.cleandoc() behavior:
/// 1. Trim leading/trailing whitespace from the entire string
/// 2. Find minimum indentation of non-empty lines (skip first line)
/// 3. Remove that indentation from all lines
///
/// # Examples
/// ```
/// # use pyo3_stub_gen::generate::normalize_docstring;
/// let doc = r#"
///     First line
///     Second line
///         Indented line
/// "#;
/// let normalized = normalize_docstring(doc);
/// assert_eq!(normalized, "First line\nSecond line\n    Indented line");
/// ```
pub fn normalize_docstring(doc: &str) -> String {
    let doc = doc.trim();
    if doc.is_empty() {
        return String::new();
    }

    let lines: Vec<&str> = doc.lines().collect();

    // Find minimum indentation of non-empty lines (skip first line)
    let min_indent = lines
        .iter()
        .skip(1)
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.chars().take_while(|c| c.is_whitespace()).count())
        .min()
        .unwrap_or(0);

    // Build normalized lines with dedenting applied
    let normalized_lines: Vec<String> = lines
        .iter()
        .enumerate()
        .map(|(i, line)| {
            if i == 0 {
                // First line: use as-is (already trimmed by outer trim())
                line.to_string()
            } else if line.trim().is_empty() {
                // Empty line: keep it but remove whitespace
                String::new()
            } else {
                // Other lines: remove common indentation
                if line.len() >= min_indent {
                    line[min_indent..].to_string()
                } else {
                    line.trim_start().to_string()
                }
            }
        })
        .collect();

    normalized_lines.join("\n")
}

pub fn write_docstring(f: &mut fmt::Formatter, doc: &str, indent: &str) -> fmt::Result {
    // Docstrings should already be normalized, but trim again for safety
    let doc = doc.trim();
    if !doc.is_empty() {
        writeln!(f, r#"{indent}r""""#)?;
        for line in doc.lines() {
            writeln!(f, "{indent}{line}")?;
        }
        writeln!(f, r#"{indent}""""#)?;
    }
    Ok(())
}
