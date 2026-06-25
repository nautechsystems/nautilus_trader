use crate::{
    generate::Import,
    stub_type::ImportRef,
    type_info::{ParameterDefault as ParameterDefaultInfo, ParameterInfo, ParameterKind},
    TypeInfo,
};
use std::{collections::HashSet, fmt};

/// Default value of a parameter (runtime version)
#[derive(Debug, Clone, PartialEq)]
pub enum ParameterDefault {
    /// No default value
    None,
    /// Default value expression as a string
    Expr(String),
}

/// A single parameter in a Python function/method signature
///
/// This struct represents a parameter at runtime during stub generation.
#[derive(Debug, Clone, PartialEq)]
pub struct Parameter {
    /// Parameter name
    pub name: &'static str,
    /// Parameter kind (positional-only, keyword-only, etc.)
    pub kind: ParameterKind,
    /// Type information
    pub type_info: TypeInfo,
    /// Default value
    pub default: ParameterDefault,
}

impl Import for Parameter {
    fn import(&self) -> HashSet<ImportRef> {
        self.type_info.import.clone()
    }
}

impl From<&ParameterInfo> for Parameter {
    fn from(info: &ParameterInfo) -> Self {
        Self {
            name: info.name,
            kind: info.kind,
            type_info: (info.type_info)(),
            default: match &info.default {
                ParameterDefaultInfo::None => ParameterDefault::None,
                ParameterDefaultInfo::Expr(f) => ParameterDefault::Expr(f()),
            },
        }
    }
}

impl fmt::Display for Parameter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self.kind {
            ParameterKind::VarPositional => {
                write!(f, "*{}: {}", self.name, self.type_info)
            }
            ParameterKind::VarKeyword => {
                write!(f, "**{}: {}", self.name, self.type_info)
            }
            _ => {
                write!(f, "{}: {}", self.name, self.type_info)?;
                match &self.default {
                    ParameterDefault::None => Ok(()),
                    ParameterDefault::Expr(expr) => write!(f, " = {}", expr),
                }
            }
        }
    }
}

/// Container for parameters in a Python function/method signature
///
/// This struct organizes parameters into sections according to Python's signature syntax,
/// ensuring proper ordering and placement of delimiters (`/` and `*`).
#[derive(Debug, Clone, PartialEq)]
pub struct Parameters {
    /// Positional-only parameters (before `/`)
    pub positional_only: Vec<Parameter>,
    /// Positional or keyword parameters
    pub positional_or_keyword: Vec<Parameter>,
    /// Keyword-only parameters (after `*`)
    pub keyword_only: Vec<Parameter>,
    /// Variable positional parameter (`*args`)
    pub varargs: Option<Parameter>,
    /// Variable keyword parameter (`**kwargs`)
    pub varkw: Option<Parameter>,
}

impl Parameters {
    /// Create an empty Parameters container
    pub fn new() -> Self {
        Self {
            positional_only: Vec::new(),
            positional_or_keyword: Vec::new(),
            keyword_only: Vec::new(),
            varargs: None,
            varkw: None,
        }
    }

    /// Build Parameters from a slice of ParameterInfo
    pub fn from_infos(infos: &[ParameterInfo]) -> Self {
        let mut params = Self::new();

        for info in infos {
            let param = Parameter::from(info);
            match param.kind {
                ParameterKind::PositionalOnly => params.positional_only.push(param),
                ParameterKind::PositionalOrKeyword => params.positional_or_keyword.push(param),
                ParameterKind::KeywordOnly => params.keyword_only.push(param),
                ParameterKind::VarPositional => params.varargs = Some(param),
                ParameterKind::VarKeyword => params.varkw = Some(param),
            }
        }

        params
    }

    /// Iterate over all parameters in signature order
    pub fn iter_entries(&self) -> impl Iterator<Item = &Parameter> {
        self.positional_only
            .iter()
            .chain(self.positional_or_keyword.iter())
            .chain(self.varargs.iter())
            .chain(self.keyword_only.iter())
            .chain(self.varkw.iter())
    }

    /// Check if there are no parameters at all
    pub fn is_empty(&self) -> bool {
        self.positional_only.is_empty()
            && self.positional_or_keyword.is_empty()
            && self.keyword_only.is_empty()
            && self.varargs.is_none()
            && self.varkw.is_none()
    }
}

impl Default for Parameters {
    fn default() -> Self {
        Self::new()
    }
}

impl Import for Parameters {
    fn import(&self) -> HashSet<ImportRef> {
        self.iter_entries().flat_map(|p| p.import()).collect()
    }
}

impl fmt::Display for Parameters {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let mut parts = Vec::new();

        // Positional-only parameters
        for param in &self.positional_only {
            parts.push(param.to_string());
        }

        // Insert `/` delimiter if there are positional-only parameters
        if !self.positional_only.is_empty() {
            parts.push("/".to_string());
        }

        // Positional or keyword parameters
        for param in &self.positional_or_keyword {
            parts.push(param.to_string());
        }

        // Variable positional parameter or bare `*` for keyword-only
        if let Some(varargs) = &self.varargs {
            parts.push(varargs.to_string());
        } else if !self.keyword_only.is_empty() {
            // Need bare `*` to indicate keyword-only parameters follow
            parts.push("*".to_string());
        }

        // Keyword-only parameters
        for param in &self.keyword_only {
            parts.push(param.to_string());
        }

        // Variable keyword parameter
        if let Some(varkw) = &self.varkw {
            parts.push(varkw.to_string());
        }

        write!(f, "{}", parts.join(", "))
    }
}

impl Parameters {
    /// Format parameters with module-qualified type names
    ///
    /// This method uses the target module context to qualify type identifiers
    /// within compound type expressions based on their source modules.
    pub fn fmt_for_module(&self, target_module: &str) -> String {
        let mut parts = Vec::new();

        // Positional-only parameters
        for param in &self.positional_only {
            parts.push(Self::format_param(param, target_module));
        }

        // Insert `/` delimiter if there are positional-only parameters
        if !self.positional_only.is_empty() {
            parts.push("/".to_string());
        }

        // Positional or keyword parameters
        for param in &self.positional_or_keyword {
            parts.push(Self::format_param(param, target_module));
        }

        // Variable positional parameter or bare `*` for keyword-only
        if let Some(varargs) = &self.varargs {
            parts.push(Self::format_param(varargs, target_module));
        } else if !self.keyword_only.is_empty() {
            // Need bare `*` to indicate keyword-only parameters follow
            parts.push("*".to_string());
        }

        // Keyword-only parameters
        for param in &self.keyword_only {
            parts.push(Self::format_param(param, target_module));
        }

        // Variable keyword parameter
        if let Some(varkw) = &self.varkw {
            parts.push(Self::format_param(varkw, target_module));
        }

        parts.join(", ")
    }

    /// Format a single parameter with qualified type names
    fn format_param(param: &Parameter, target_module: &str) -> String {
        let qualified_type = param.type_info.qualified_for_module(target_module);
        match param.kind {
            ParameterKind::VarPositional => format!("*{}: {}", param.name, qualified_type),
            ParameterKind::VarKeyword => format!("**{}: {}", param.name, qualified_type),
            _ => {
                let base = format!("{}: {}", param.name, qualified_type);
                match &param.default {
                    ParameterDefault::None => base,
                    ParameterDefault::Expr(expr) => format!("{} = {}", base, expr),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_positional_only() {
        let params = Parameters {
            positional_only: vec![
                Parameter {
                    name: "x",
                    kind: ParameterKind::PositionalOnly,
                    type_info: TypeInfo::builtin("int"),
                    default: ParameterDefault::None,
                },
                Parameter {
                    name: "y",
                    kind: ParameterKind::PositionalOnly,
                    type_info: TypeInfo::builtin("int"),
                    default: ParameterDefault::None,
                },
            ],
            ..Default::default()
        };

        assert_eq!(params.to_string(), "x: builtins.int, y: builtins.int, /");
    }

    #[test]
    fn test_keyword_only() {
        let params = Parameters {
            keyword_only: vec![Parameter {
                name: "kw",
                kind: ParameterKind::KeywordOnly,
                type_info: TypeInfo::builtin("str"),
                default: ParameterDefault::Expr("None".to_string()),
            }],
            ..Default::default()
        };

        assert_eq!(params.to_string(), "*, kw: builtins.str = None");
    }

    #[test]
    fn test_mixed_with_defaults() {
        let params = Parameters {
            positional_only: vec![Parameter {
                name: "token",
                kind: ParameterKind::PositionalOnly,
                type_info: TypeInfo::builtin("str"),
                default: ParameterDefault::None,
            }],
            keyword_only: vec![
                Parameter {
                    name: "retries",
                    kind: ParameterKind::KeywordOnly,
                    type_info: TypeInfo::builtin("int"),
                    default: ParameterDefault::Expr("3".to_string()),
                },
                Parameter {
                    name: "timeout",
                    kind: ParameterKind::KeywordOnly,
                    type_info: TypeInfo::builtin("float"),
                    default: ParameterDefault::None,
                },
            ],
            ..Default::default()
        };

        assert_eq!(
            params.to_string(),
            "token: builtins.str, /, *, retries: builtins.int = 3, timeout: builtins.float"
        );
    }

    #[test]
    fn test_varargs_kwargs() {
        let params = Parameters {
            varargs: Some(Parameter {
                name: "args",
                kind: ParameterKind::VarPositional,
                type_info: TypeInfo::builtin("str"),
                default: ParameterDefault::None,
            }),
            varkw: Some(Parameter {
                name: "kwargs",
                kind: ParameterKind::VarKeyword,
                type_info: TypeInfo::any(),
                default: ParameterDefault::None,
            }),
            ..Default::default()
        };

        assert_eq!(
            params.to_string(),
            "*args: builtins.str, **kwargs: typing.Any"
        );
    }
}
