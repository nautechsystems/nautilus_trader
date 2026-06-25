use crate::generate::docstring::normalize_docstring;
use crate::{generate::*, type_info::*};
use std::fmt;

/// Definition of a Python enum.
#[derive(Debug, Clone, PartialEq)]
pub struct EnumDef {
    pub name: &'static str,
    pub module: Option<&'static str>,
    pub doc: &'static str,
    pub variants: &'static [(&'static str, &'static str)],
    pub methods: Vec<MethodDef>,
    pub attrs: Vec<MemberDef>,
    pub getters: Vec<MemberDef>,
    pub setters: Vec<MemberDef>,
}

impl From<&PyEnumInfo> for EnumDef {
    fn from(info: &PyEnumInfo) -> Self {
        let doc = if info.doc.is_empty() {
            ""
        } else {
            Box::leak(normalize_docstring(info.doc).into_boxed_str())
        };

        // Normalize variant docstrings
        let variants_vec: Vec<(&'static str, &'static str)> = info
            .variants
            .iter()
            .map(|(name, variant_doc)| {
                let normalized_variant_doc = if variant_doc.is_empty() {
                    ""
                } else {
                    Box::leak(normalize_docstring(variant_doc).into_boxed_str())
                };
                (*name, normalized_variant_doc)
            })
            .collect();
        let variants: &'static [(&'static str, &'static str)] =
            Box::leak(variants_vec.into_boxed_slice());

        Self {
            name: info.pyclass_name,
            module: info.module,
            doc,
            variants,
            methods: Vec::new(),
            attrs: Vec::new(),
            getters: Vec::new(),
            setters: Vec::new(),
        }
    }
}

impl Import for EnumDef {
    fn import(&self) -> HashSet<ImportRef> {
        let mut import = HashSet::new();
        // for @typing.final
        import.insert("typing".into());
        // for Enum base class
        import.insert("enum".into());
        for method in &self.methods {
            import.extend(method.import());
        }
        for attr in &self.attrs {
            import.extend(attr.import());
        }
        for getter in &self.getters {
            import.extend(getter.import());
        }
        for setter in &self.setters {
            import.extend(setter.import());
        }
        import
    }
}

impl fmt::Display for EnumDef {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "@typing.final")?;
        writeln!(f, "class {}(enum.Enum):", self.name)?;
        let indent = indent();
        docstring::write_docstring(f, self.doc, indent)?;
        for (variant, variant_doc) in self.variants {
            writeln!(f, "{indent}{variant} = ...")?;
            docstring::write_docstring(f, variant_doc, indent)?;
        }
        if !(self.attrs.is_empty()
            && self.getters.is_empty()
            && self.setters.is_empty()
            && self.methods.is_empty())
        {
            writeln!(f)?;
            for attr in &self.attrs {
                attr.fmt(f)?;
            }
            for getter in &self.getters {
                write!(
                    f,
                    "{}",
                    GetterDisplay {
                        member: getter,
                        target_module: self.module.unwrap_or(self.name)
                    }
                )?;
            }
            for setter in &self.setters {
                write!(
                    f,
                    "{}",
                    SetterDisplay {
                        member: setter,
                        target_module: self.module.unwrap_or(self.name)
                    }
                )?;
            }
            for methods in &self.methods {
                methods.fmt(f)?;
            }
        }
        writeln!(f)?;
        Ok(())
    }
}

impl EnumDef {
    /// Format enum with module-qualified type names
    ///
    /// This method uses the target module context to qualify type identifiers
    /// within compound type expressions based on their source modules.
    /// Note: Enums currently don't have TypeInfo in their base classes, so this
    /// mostly delegates to Display, but is provided for API consistency.
    pub fn fmt_for_module(&self, target_module: &str, f: &mut fmt::Formatter) -> fmt::Result {
        writeln!(f, "@typing.final")?;
        writeln!(f, "class {}(enum.Enum):", self.name)?;
        let indent = indent();
        docstring::write_docstring(f, self.doc, indent)?;
        for (variant, variant_doc) in self.variants {
            writeln!(f, "{indent}{variant} = ...")?;
            docstring::write_docstring(f, variant_doc, indent)?;
        }
        if !(self.attrs.is_empty()
            && self.getters.is_empty()
            && self.setters.is_empty()
            && self.methods.is_empty())
        {
            writeln!(f)?;
            for attr in &self.attrs {
                attr.fmt_for_module(target_module, f, indent)?;
            }
            for getter in &self.getters {
                write!(
                    f,
                    "{}",
                    GetterDisplay {
                        member: getter,
                        target_module
                    }
                )?;
            }
            for setter in &self.setters {
                write!(
                    f,
                    "{}",
                    SetterDisplay {
                        member: setter,
                        target_module
                    }
                )?;
            }
            for methods in &self.methods {
                methods.fmt_for_module(target_module, f, indent)?;
            }
        }
        writeln!(f)?;
        Ok(())
    }
}
