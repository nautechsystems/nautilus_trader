use crate::generate::docstring::normalize_docstring;
use crate::stub_type::ImportRef;
use crate::{generate::*, rule_name::RuleName, type_info::*, TypeInfo};
use itertools::Itertools;
use std::{collections::HashSet, fmt};

pub use crate::type_info::MethodType;

/// Definition of a class method.
#[derive(Debug, Clone, PartialEq)]
pub struct MethodDef {
    pub name: &'static str,
    pub parameters: Parameters,
    pub r#return: TypeInfo,
    pub doc: &'static str,
    pub r#type: MethodType,
    pub is_async: bool,
    pub deprecated: Option<DeprecatedInfo>,
    pub type_ignored: Option<IgnoreTarget>,
    /// Whether this method is marked as an overload variant
    pub is_overload: bool,
}

impl Import for MethodDef {
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

impl From<&MethodInfo> for MethodDef {
    fn from(info: &MethodInfo) -> Self {
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
            r#type: info.r#type,
            is_async: info.is_async,
            deprecated: info.deprecated.clone(),
            type_ignored: info.type_ignored,
            is_overload: info.is_overload,
        }
    }
}

impl fmt::Display for MethodDef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let indent = indent();
        let async_ = if self.is_async { "async " } else { "" };

        // Add deprecated decorator if present
        if let Some(deprecated) = &self.deprecated {
            writeln!(f, "{indent}{deprecated}")?;
        }

        let params_str = if self.parameters.is_empty() {
            String::new()
        } else {
            format!(", {}", self.parameters)
        };

        match self.r#type {
            MethodType::Static => {
                writeln!(f, "{indent}@staticmethod")?;
                write!(f, "{indent}{async_}def {}({})", self.name, self.parameters)?;
            }
            MethodType::Class | MethodType::New => {
                if self.r#type == MethodType::Class {
                    // new is a classmethod without the decorator
                    writeln!(f, "{indent}@classmethod")?;
                }
                write!(f, "{indent}{async_}def {}(cls{})", self.name, params_str)?;
            }
            MethodType::Instance => {
                write!(f, "{indent}{async_}def {}(self{})", self.name, params_str)?;
            }
        }
        write!(f, " -> {}:", self.r#return)?;

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
            // Add type: ignore comment for methods with docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
            let double_indent = format!("{indent}{indent}");
            docstring::write_docstring(f, self.doc, &double_indent)?;
        } else {
            write!(f, " ...")?;
            // Add type: ignore comment for methods without docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}

impl MethodDef {
    /// Format method with module-qualified type names
    ///
    /// This method uses the target module context to qualify type identifiers
    /// within compound type expressions based on their source modules.
    pub fn fmt_for_module(
        &self,
        target_module: &str,
        f: &mut fmt::Formatter,
        indent: &str,
    ) -> fmt::Result {
        let async_ = if self.is_async { "async " } else { "" };

        // Add deprecated decorator if present
        if let Some(deprecated) = &self.deprecated {
            writeln!(f, "{indent}{deprecated}")?;
        }

        let params_str = self.parameters.fmt_for_module(target_module);
        let return_type = self.r#return.qualified_for_module(target_module);

        let params_with_separator = if params_str.is_empty() {
            String::new()
        } else {
            format!(", {}", params_str)
        };

        match self.r#type {
            MethodType::Static => {
                writeln!(f, "{indent}@staticmethod")?;
                write!(f, "{indent}{async_}def {}({})", self.name, params_str)?;
            }
            MethodType::Class | MethodType::New => {
                if self.r#type == MethodType::Class {
                    // new is a classmethod without the decorator
                    writeln!(f, "{indent}@classmethod")?;
                }
                write!(
                    f,
                    "{indent}{async_}def {}(cls{})",
                    self.name, params_with_separator
                )?;
            }
            MethodType::Instance => {
                write!(
                    f,
                    "{indent}{async_}def {}(self{})",
                    self.name, params_with_separator
                )?;
            }
        }
        write!(f, " -> {}:", return_type)?;

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
            // Add type: ignore comment for methods with docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
            let double_indent = format!("{indent}{indent}");
            docstring::write_docstring(f, self.doc, &double_indent)?;
        } else {
            write!(f, " ...")?;
            // Add type: ignore comment for methods without docstrings
            if let Some(comment) = &type_ignore_comment {
                write!(f, "{comment}")?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
}
