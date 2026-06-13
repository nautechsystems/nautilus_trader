use crate::generate::docstring::normalize_docstring;
use crate::stub_type::ImportRef;
use crate::{generate::*, rule_name::RuleName, type_info::*, TypeInfo};
use itertools::Itertools;
use std::fmt;

/// Definition of a Python function.
#[derive(Debug, Clone, PartialEq)]
pub struct FunctionDef {
    pub name: &'static str,
    pub parameters: Parameters,
    pub r#return: TypeInfo,
    pub doc: &'static str,
    pub is_async: bool,
    pub deprecated: Option<DeprecatedInfo>,
    pub type_ignored: Option<IgnoreTarget>,
    pub is_overload: bool,
    /// Source file location for deterministic ordering
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
    /// Index for ordering multiple functions from the same macro invocation
    pub index: usize,
}

impl Import for FunctionDef {
    fn import(&self) -> HashSet<ImportRef> {
        let mut import = self.r#return.import.clone();
        import.extend(self.parameters.import());
        // Add typing_extensions import if deprecated
        if self.deprecated.is_some() {
            import.insert("typing_extensions".into());
        }
        import
    }
}

impl From<&PyFunctionInfo> for FunctionDef {
    fn from(info: &PyFunctionInfo) -> Self {
        let doc = if info.doc.is_empty() {
            ""
        } else {
            Box::leak(normalize_docstring(info.doc).into_boxed_str())
        };

        Self {
            name: info.name,
            parameters: Parameters::from_infos(info.parameters),
            r#return: (info.r#return)(),
            doc,
            is_async: info.is_async,
            deprecated: info.deprecated.clone(),
            type_ignored: info.type_ignored,
            is_overload: info.is_overload,
            file: info.file,
            line: info.line,
            column: info.column,
            index: info.index,
        }
    }
}

impl fmt::Display for FunctionDef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        // Add deprecated decorator if present
        if let Some(deprecated) = &self.deprecated {
            writeln!(f, "{deprecated}")?;
        }

        let async_ = if self.is_async { "async " } else { "" };
        write!(
            f,
            "{async_}def {}({}) -> {}:",
            self.name, self.parameters, self.r#return
        )?;

        // Calculate type: ignore comment once
        let type_ignore_comment = if let Some(target) = &self.type_ignored {
            match target {
                IgnoreTarget::All => Some("  # type: ignore".to_string()),
                IgnoreTarget::Specified(rules) => {
                    let rules_str = rules
                        .iter()
                        .map(|r| {
                            let result = r.parse::<RuleName>().unwrap();
                            if let RuleName::Custom(custom) = &result {
                                log::warn!("Unknown custom rule name '{custom}' used in type ignore. Ensure this is intended.");
                            }
                            result
                        })
                        .join(",");
                    Some(format!("  # type: ignore[{rules_str}]"))
                }
            }
        } else {
            None
        };

        let doc = self.doc;
        if !doc.is_empty() {
            // Add type: ignore comment for functions with docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
            docstring::write_docstring(f, self.doc, indent())?;
        } else {
            write!(f, " ...")?;
            // Add type: ignore comment for functions without docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
        }
        writeln!(f)?;
        Ok(())
    }
}

impl FunctionDef {
    /// Resolve all ModuleRef::Default to actual module name.
    /// Called after construction, before formatting.
    pub fn resolve_default_modules(&mut self, default_module_name: &str) {
        // Resolve all parameter types
        for param in &mut self.parameters.positional_only {
            param.type_info.resolve_default_module(default_module_name);
        }
        for param in &mut self.parameters.positional_or_keyword {
            param.type_info.resolve_default_module(default_module_name);
        }
        for param in &mut self.parameters.keyword_only {
            param.type_info.resolve_default_module(default_module_name);
        }
        if let Some(varargs) = &mut self.parameters.varargs {
            varargs
                .type_info
                .resolve_default_module(default_module_name);
        }
        if let Some(varkw) = &mut self.parameters.varkw {
            varkw.type_info.resolve_default_module(default_module_name);
        }
        self.r#return.resolve_default_module(default_module_name);
    }
}

impl FunctionDef {
    /// Format function with module-qualified type names
    ///
    /// This method uses the target module context to qualify type identifiers
    /// within compound type expressions based on their source modules.
    pub fn fmt_for_module(&self, target_module: &str, f: &mut fmt::Formatter) -> fmt::Result {
        // Add deprecated decorator if present
        if let Some(deprecated) = &self.deprecated {
            writeln!(f, "{deprecated}")?;
        }

        let async_ = if self.is_async { "async " } else { "" };
        let params_str = self.parameters.fmt_for_module(target_module);
        let return_type = self.r#return.qualified_for_module(target_module);

        write!(
            f,
            "{async_}def {}({}) -> {}:",
            self.name, params_str, return_type
        )?;

        // Calculate type: ignore comment once
        let type_ignore_comment = if let Some(target) = &self.type_ignored {
            match target {
                IgnoreTarget::All => Some("  # type: ignore".to_string()),
                IgnoreTarget::Specified(rules) => {
                    let rules_str = rules
                        .iter()
                        .map(|r| {
                            let result = r.parse::<RuleName>().unwrap();
                            if let RuleName::Custom(custom) = &result {
                                log::warn!("Unknown custom rule name '{custom}' used in type ignore. Ensure this is intended.");
                            }
                            result
                        })
                        .join(",");
                    Some(format!("  # type: ignore[{rules_str}]"))
                }
            }
        } else {
            None
        };

        let doc = self.doc;
        if !doc.is_empty() {
            // Add type: ignore comment for functions with docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
            docstring::write_docstring(f, self.doc, indent())?;
        } else {
            write!(f, " ...")?;
            // Add type: ignore comment for functions without docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
        }
        writeln!(f)?;
        Ok(())
    }
}
