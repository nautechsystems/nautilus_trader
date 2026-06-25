use std::{collections::HashSet, fmt};

use crate::{generate::Import, stub_type::ImportRef, type_info::PyVariableInfo, TypeInfo};

#[derive(Debug, Clone, PartialEq)]
pub struct VariableDef {
    pub name: &'static str,
    pub type_: TypeInfo,
    pub default: Option<String>,
}

impl From<&PyVariableInfo> for VariableDef {
    fn from(info: &PyVariableInfo) -> Self {
        Self {
            name: info.name,
            type_: (info.r#type)(),
            default: info.default.map(|f| f()),
        }
    }
}

impl Import for VariableDef {
    fn import(&self) -> HashSet<ImportRef> {
        self.type_.import.clone()
    }
}

impl fmt::Display for VariableDef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}: {}", self.name, self.type_)?;
        if let Some(default) = &self.default {
            write!(f, " = {default}")?;
        }
        Ok(())
    }
}

impl VariableDef {
    /// Format variable with module-qualified type names
    ///
    /// This method uses the target module context to qualify type identifiers
    /// within compound type expressions based on their source modules.
    pub fn fmt_for_module(&self, target_module: &str, f: &mut fmt::Formatter) -> fmt::Result {
        let qualified_type = self.type_.qualified_for_module(target_module);
        write!(f, "{}: {}", self.name, qualified_type)?;
        if let Some(default) = &self.default {
            write!(f, " = {default}")?;
        }
        Ok(())
    }
}
