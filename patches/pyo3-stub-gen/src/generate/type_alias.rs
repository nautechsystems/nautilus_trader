use std::{collections::HashSet, fmt};

use crate::{
    generate::{docstring, Import},
    stub_type::ImportRef,
    type_info::TypeAliasInfo,
    TypeInfo,
};

#[derive(Debug, Clone, PartialEq)]
pub struct TypeAliasDef {
    pub name: &'static str,
    pub type_: TypeInfo,
    pub doc: &'static str,
}

impl From<&TypeAliasInfo> for TypeAliasDef {
    fn from(info: &TypeAliasInfo) -> Self {
        Self {
            name: info.name,
            type_: (info.r#type)(),
            doc: info.doc,
        }
    }
}

impl Import for TypeAliasDef {
    fn import(&self) -> HashSet<ImportRef> {
        // Only return imports from the type itself
        // TypeAlias will be handled conditionally by Module
        self.type_.import.clone()
    }
}

impl fmt::Display for TypeAliasDef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: TypeAlias = {}", self.name, self.type_)
    }
}

impl TypeAliasDef {
    /// Format type alias with module-qualified names and syntax based on configuration
    pub fn fmt_with_config(
        &self,
        target_module: &str,
        f: &mut fmt::Formatter,
        use_type_statement: bool,
    ) -> fmt::Result {
        let qualified_type = self.type_.qualified_for_module(target_module);

        if use_type_statement {
            // Python 3.12+ syntax
            write!(f, "type {} = {}", self.name, qualified_type)?;
        } else {
            // Pre-3.12 syntax (default)
            write!(f, "{}: TypeAlias = {}", self.name, qualified_type)?;
        }

        // Add docstring on next line if present
        if !self.doc.is_empty() {
            writeln!(f)?;
            docstring::write_docstring(f, self.doc, "")?;
        }
        Ok(())
    }

    /// Existing method for backward compatibility (uses pre-3.12 syntax)
    pub fn fmt_for_module(&self, target_module: &str, f: &mut fmt::Formatter) -> fmt::Result {
        self.fmt_with_config(target_module, f, false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fmt::Write;

    #[test]
    fn test_pre_312_syntax() {
        let alias = TypeAliasDef {
            name: "MyAlias",
            type_: TypeInfo::builtin("int"),
            doc: "",
        };
        let mut output = String::new();
        write!(
            &mut output,
            "{}",
            FormatterWrapper(&alias, "test_module", false)
        )
        .unwrap();
        assert!(output.contains("MyAlias: TypeAlias = builtins.int"));
    }

    #[test]
    fn test_312_syntax() {
        let alias = TypeAliasDef {
            name: "MyAlias",
            type_: TypeInfo::builtin("int"),
            doc: "",
        };
        let mut output = String::new();
        write!(
            &mut output,
            "{}",
            FormatterWrapper(&alias, "test_module", true)
        )
        .unwrap();
        assert!(output.contains("type MyAlias = builtins.int"));
        assert!(!output.contains("TypeAlias"));
    }

    // Helper struct to test formatting
    struct FormatterWrapper<'a>(&'a TypeAliasDef, &'a str, bool);

    impl<'a> fmt::Display for FormatterWrapper<'a> {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            self.0.fmt_with_config(self.1, f, self.2)
        }
    }
}
