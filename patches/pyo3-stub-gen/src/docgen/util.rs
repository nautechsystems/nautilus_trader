//! Utility functions for documentation generation

/// Module for prefix stripping utilities
pub mod prefix_stripper {
    /// Strip standard library prefixes from type expressions
    ///
    /// Removes prefixes like "typing.", "builtins.", "collections.abc.", etc.
    /// while preserving the structure of complex type expressions.
    pub fn strip_stdlib_prefixes(type_expr: &str) -> String {
        let stdlib_prefixes = &[
            "typing.",
            "builtins.",
            "collections.abc.",
            "typing_extensions.",
            "decimal.",
            "datetime.",
            "pathlib.",
        ];

        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = type_expr.chars().collect();

        while i < chars.len() {
            // Check if we're at the start of a qualified name
            if i == 0
                || !chars[i - 1].is_alphanumeric() && chars[i - 1] != '_' && chars[i - 1] != '.'
            {
                let remaining: String = chars[i..].iter().collect();
                let mut matched = false;

                // Try to match stdlib prefixes
                for prefix in stdlib_prefixes {
                    if remaining.starts_with(prefix) {
                        let after_prefix_idx = prefix.len();
                        if after_prefix_idx < remaining.len() {
                            let next_char = remaining.chars().nth(after_prefix_idx).unwrap();
                            if next_char.is_alphabetic() || next_char == '_' {
                                i += prefix.len();
                                matched = true;
                                break;
                            }
                        }
                    }
                }

                if matched {
                    continue;
                }
            }

            result.push(chars[i]);
            i += 1;
        }

        result
    }

    /// Strip package-qualified names from type expressions
    ///
    /// Converts "package.Type" -> "Type" based on heuristics:
    /// - First part starts with lowercase (likely a module)
    /// - Last part starts with uppercase (likely a type)
    pub fn strip_package_prefixes(type_expr: &str, _current_module: &str) -> String {
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = type_expr.chars().collect();

        while i < chars.len() {
            // Check if we're at the start of a qualified name
            if i == 0
                || !chars[i - 1].is_alphanumeric() && chars[i - 1] != '_' && chars[i - 1] != '.'
            {
                let remaining: String = chars[i..].iter().collect();

                // Extract qualified identifier (e.g., "main_mod.A" or "pure.DataContainer")
                let ident_match = remaining
                    .split(|c: char| !c.is_alphanumeric() && c != '_' && c != '.')
                    .next();

                if let Some(ident) = ident_match {
                    if ident.contains('.') {
                        // This is a qualified name, check if it's a package.Type pattern
                        let parts: Vec<&str> = ident.split('.').collect();
                        if parts.len() >= 2 {
                            let first_part = parts[0];
                            let last_part = parts[parts.len() - 1];

                            // If it looks like a package.Type pattern, extract just the Type
                            if first_part
                                .chars()
                                .next()
                                .map(|c| c.is_lowercase())
                                .unwrap_or(false)
                                && last_part
                                    .chars()
                                    .next()
                                    .map(|c| c.is_uppercase())
                                    .unwrap_or(false)
                            {
                                // Skip to the last part
                                let prefix_len = ident.len() - last_part.len();
                                i += prefix_len;
                                continue;
                            }
                        }
                    }
                }
            }

            result.push(chars[i]);
            i += 1;
        }

        result
    }

    /// Strip internal module prefixes (modules starting with underscore)
    ///
    /// Converts "_core.Type" -> "Type", "_internal._nested.Type" -> "Type"
    pub fn strip_internal_prefixes(expr: &str) -> String {
        let mut result = String::new();
        let mut i = 0;
        let chars: Vec<char> = expr.chars().collect();

        while i < chars.len() {
            // Check if we're at the start of a potential module prefix
            if i == 0 || !chars[i - 1].is_alphanumeric() && chars[i - 1] != '_' {
                // Try to match a pattern like "_module." or "_module.submodule."
                let remaining: String = chars[i..].iter().collect();

                // Look for pattern: _identifier followed by .
                if remaining.starts_with('_') {
                    // Find the next non-identifier character
                    let mut j = i + 1;
                    while j < chars.len() && (chars[j].is_alphanumeric() || chars[j] == '_') {
                        j += 1;
                    }

                    // If followed by a dot, this is a module prefix to strip
                    if j < chars.len() && chars[j] == '.' {
                        // Skip the module name and the dot
                        i = j + 1;
                        continue;
                    }
                }
            }

            result.push(chars[i]);
            i += 1;
        }

        result
    }

    /// Combined stripper for type expressions (stdlib + package prefixes)
    ///
    /// This is the main entry point for stripping prefixes from type expressions.
    pub fn strip_type_prefixes(type_expr: &str, current_module: &str) -> String {
        let without_stdlib = strip_stdlib_prefixes(type_expr);
        strip_package_prefixes(&without_stdlib, current_module)
    }

    /// Simple prefix stripper for default values
    ///
    /// Uses simple string replacement for common prefixes.
    /// This is faster but less sophisticated than strip_type_prefixes.
    pub fn strip_default_value_prefixes(text: &str) -> String {
        text.replace("_core.", "")
            .replace("typing.", "")
            .replace("builtins.", "")
    }
}

/// Check if module is hidden (any path component starts with '_')
pub fn is_hidden_module(module_name: &str) -> bool {
    module_name.split('.').any(|part| part.starts_with('_'))
}

#[cfg(test)]
mod tests {
    use super::prefix_stripper::*;
    use super::*;

    #[test]
    fn test_strip_stdlib_prefixes() {
        assert_eq!(strip_stdlib_prefixes("typing.Optional"), "Optional");
        assert_eq!(strip_stdlib_prefixes("builtins.str"), "str");
        assert_eq!(
            strip_stdlib_prefixes("collections.abc.Callable"),
            "Callable"
        );
        assert_eq!(
            strip_stdlib_prefixes("typing.Optional[typing.List[int]]"),
            "Optional[List[int]]"
        );
    }

    #[test]
    fn test_strip_package_prefixes() {
        assert_eq!(strip_package_prefixes("main_mod.ClassA", ""), "ClassA");
        assert_eq!(
            strip_package_prefixes("pure.DataContainer", ""),
            "DataContainer"
        );
        assert_eq!(
            strip_package_prefixes("Optional[main_mod.ClassA]", ""),
            "Optional[ClassA]"
        );
    }

    #[test]
    fn test_strip_internal_prefixes() {
        assert_eq!(
            strip_internal_prefixes("_core.InternalType"),
            "InternalType"
        );
        assert_eq!(strip_internal_prefixes("_internal._nested.Type"), "Type");
        assert_eq!(
            strip_internal_prefixes("Optional[_core.Type]"),
            "Optional[Type]"
        );
    }

    #[test]
    fn test_strip_type_prefixes() {
        assert_eq!(
            strip_type_prefixes("typing.Optional[main_mod.ClassA]", "main_mod"),
            "Optional[ClassA]"
        );
    }

    #[test]
    fn test_strip_default_value_prefixes() {
        assert_eq!(strip_default_value_prefixes("_core.Foo"), "Foo");
        assert_eq!(strip_default_value_prefixes("typing.Optional"), "Optional");
        assert_eq!(strip_default_value_prefixes("builtins.None"), "None");
    }

    #[test]
    fn test_is_hidden_module() {
        assert!(is_hidden_module("_core"));
        assert!(is_hidden_module("package._internal"));
        assert!(is_hidden_module("_hidden.submodule"));
        assert!(!is_hidden_module("public"));
        assert!(!is_hidden_module("package.submodule"));
    }
}
