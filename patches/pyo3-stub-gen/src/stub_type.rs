mod builtins;
mod collections;
mod pyo3;

#[cfg(feature = "numpy")]
mod numpy;

#[cfg(feature = "either")]
mod either;

#[cfg(feature = "rust_decimal")]
mod rust_decimal;

use maplit::hashset;
use std::cmp::Ordering;
use std::{
    collections::{HashMap, HashSet},
    fmt, ops,
};

/// Indicates what to import.
/// Module: The purpose is to import the entire module(eg import builtins).
/// Type: The purpose is to import the types in the module(eg from moduleX import typeX).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ImportRef {
    Module(ModuleRef),
    Type(TypeRef),
}

impl From<&str> for ImportRef {
    fn from(value: &str) -> Self {
        ImportRef::Module(value.into())
    }
}

impl PartialOrd for ImportRef {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ImportRef {
    fn cmp(&self, other: &Self) -> Ordering {
        match (self, other) {
            (ImportRef::Module(a), ImportRef::Module(b)) => a.get().cmp(&b.get()),
            (ImportRef::Type(a), ImportRef::Type(b)) => a.cmp(b),
            (ImportRef::Module(_), ImportRef::Type(_)) => Ordering::Greater,
            (ImportRef::Type(_), ImportRef::Module(_)) => Ordering::Less,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub enum ModuleRef {
    Named(String),

    /// Default module that PyO3 creates.
    ///
    /// - For pure Rust project, the default module name is the crate name specified in `Cargo.toml`
    ///   or `project.name` specified in `pyproject.toml`
    /// - For mixed Rust/Python project, the default module name is `tool.maturin.module-name` specified in `pyproject.toml`
    ///
    /// Because the default module name cannot be known at compile time, it will be resolved at the time of the stub file generation.
    /// This is a placeholder for the default module name.
    #[default]
    Default,
}

impl ModuleRef {
    pub fn get(&self) -> Option<&str> {
        match self {
            Self::Named(name) => Some(name),
            Self::Default => None,
        }
    }
}

impl From<&str> for ModuleRef {
    fn from(s: &str) -> Self {
        Self::Named(s.to_string())
    }
}

/// Indicates the type of import(eg class enum).
/// from module import type.
/// name, type name. module, module name(which type defined).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Default, Hash)]
pub struct TypeRef {
    pub module: ModuleRef,
    pub name: String,
}

impl TypeRef {
    pub fn new(module_ref: ModuleRef, name: String) -> Self {
        Self {
            module: module_ref,
            name,
        }
    }
}

/// Represents how a type identifier should be qualified in stub files.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportKind {
    /// Type is imported by name (from module import Type).
    /// It can be used unqualified in the target module.
    ByName,
    /// Type is from a module import (from package import module).
    /// It must be qualified as module.Type.
    Module,
    /// Type is defined in the same module as the usage site.
    /// It can be used unqualified.
    SameModule,
}

/// Represents a reference to a type identifier within a compound type expression.
/// Tracks which module the type comes from and how it should be qualified.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeIdentifierRef {
    /// The module where this type is defined.
    pub module: ModuleRef,
    /// How this type should be qualified in stub files.
    pub import_kind: ImportKind,
}

/// Type information for creating Python stub files annotated by [PyStubType] trait.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    /// The Python type name.
    pub name: String,

    /// The module this type belongs to.
    ///
    /// - `None`: Type has no source module (e.g., `typing.Any`, primitives, generic container types)
    /// - `Some(ModuleRef::Default)`: Type from current package's default module
    /// - `Some(ModuleRef::Named(path))`: Type from specific module (e.g., `"package.sub_mod"`)
    pub source_module: Option<ModuleRef>,

    /// Python modules must be imported in the stub file.
    ///
    /// For example, when `name` is `typing.Sequence[int]`, `import` should contain `typing`.
    /// This makes it possible to use user-defined types in the stub file.
    pub import: HashSet<ImportRef>,

    /// Track all type identifiers referenced in the name expression.
    ///
    /// This enables context-aware qualification of identifiers within compound type expressions.
    /// For example, in `typing.Optional[ClassA]`, we need to track that `ClassA` is from a specific module
    /// and qualify it appropriately based on the target module context.
    ///
    /// - Key: bare identifier (e.g., "ClassA")
    /// - Value: TypeIdentifierRef containing module and import kind
    pub type_refs: HashMap<String, TypeIdentifierRef>,
}

impl fmt::Display for TypeInfo {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.name)
    }
}

impl TypeInfo {
    /// A `None` type annotation.
    pub fn none() -> Self {
        // NOTE: since 3.10, NoneType is provided from types module,
        // but there is no corresponding definitions prior to 3.10.
        Self {
            name: "None".to_string(),
            source_module: None,
            import: HashSet::new(),
            type_refs: HashMap::new(),
        }
    }

    /// A `typing.Any` type annotation.
    pub fn any() -> Self {
        Self {
            name: "typing.Any".to_string(),
            source_module: None,
            import: hashset! { "typing".into() },
            type_refs: HashMap::new(),
        }
    }

    /// A `list[Type]` type annotation.
    pub fn list_of<T: PyStubType>() -> Self {
        let inner = T::type_output();
        let mut import = inner.import.clone();
        import.insert("builtins".into());

        // Build type_refs from inner type
        let mut type_refs = HashMap::new();
        if let Some(ref source_module) = inner.source_module {
            if let Some(_module_name) = source_module.get() {
                // Extract bare type identifier from the (potentially qualified) name
                let bare_name = inner
                    .name
                    .split('[')
                    .next()
                    .unwrap_or(&inner.name)
                    .split('.')
                    .next_back()
                    .unwrap_or(&inner.name);
                type_refs.insert(
                    bare_name.to_string(),
                    TypeIdentifierRef {
                        module: source_module.clone(),
                        import_kind: ImportKind::Module,
                    },
                );
            }
        }
        type_refs.extend(inner.type_refs);

        TypeInfo {
            name: format!("builtins.list[{}]", inner.name),
            source_module: None,
            import,
            type_refs,
        }
    }

    /// A `set[Type]` type annotation.
    pub fn set_of<T: PyStubType>() -> Self {
        let inner = T::type_output();
        let mut import = inner.import.clone();
        import.insert("builtins".into());

        // Build type_refs from inner type
        let mut type_refs = HashMap::new();
        if let Some(ref source_module) = inner.source_module {
            if let Some(_module_name) = source_module.get() {
                let bare_name = inner
                    .name
                    .split('[')
                    .next()
                    .unwrap_or(&inner.name)
                    .split('.')
                    .next_back()
                    .unwrap_or(&inner.name);
                type_refs.insert(
                    bare_name.to_string(),
                    TypeIdentifierRef {
                        module: source_module.clone(),
                        import_kind: ImportKind::Module,
                    },
                );
            }
        }
        type_refs.extend(inner.type_refs);

        TypeInfo {
            name: format!("builtins.set[{}]", inner.name),
            source_module: None,
            import,
            type_refs,
        }
    }

    /// A `dict[Type]` type annotation.
    pub fn dict_of<K: PyStubType, V: PyStubType>() -> Self {
        let inner_k = K::type_output();
        let inner_v = V::type_output();
        let mut import = inner_k.import.clone();
        import.extend(inner_v.import.clone());
        import.insert("builtins".into());

        // Build type_refs from both inner types
        let mut type_refs = HashMap::new();
        for inner in [&inner_k, &inner_v] {
            if let Some(ref source_module) = inner.source_module {
                if let Some(_module_name) = source_module.get() {
                    let bare_name = inner
                        .name
                        .split('[')
                        .next()
                        .unwrap_or(&inner.name)
                        .split('.')
                        .next_back()
                        .unwrap_or(&inner.name);
                    type_refs.insert(
                        bare_name.to_string(),
                        TypeIdentifierRef {
                            module: source_module.clone(),
                            import_kind: ImportKind::Module,
                        },
                    );
                }
            }
            type_refs.extend(inner.type_refs.clone());
        }

        TypeInfo {
            name: format!("builtins.dict[{}, {}]", inner_k.name, inner_v.name),
            source_module: None,
            import,
            type_refs,
        }
    }

    /// A type annotation of a built-in type provided from `builtins` module, such as `int`, `str`, or `float`. Generic builtin types are also possible, such as `dict[str, str]`.
    pub fn builtin(name: &str) -> Self {
        Self {
            name: format!("builtins.{name}"),
            source_module: None,
            import: hashset! { "builtins".into() },
            type_refs: HashMap::new(),
        }
    }

    /// Unqualified type.
    pub fn unqualified(name: &str) -> Self {
        Self {
            name: name.to_string(),
            source_module: None,
            import: hashset! {},
            type_refs: HashMap::new(),
        }
    }

    /// A type annotation of a type that must be imported. The type name must be qualified with the module name:
    ///
    /// ```
    /// pyo3_stub_gen::TypeInfo::with_module("pathlib.Path", "pathlib".into());
    /// ```
    pub fn with_module(name: &str, module: ModuleRef) -> Self {
        let mut import = HashSet::new();
        import.insert(ImportRef::Module(module.clone()));
        Self {
            name: name.to_string(),
            source_module: Some(module),
            import,
            type_refs: HashMap::new(),
        }
    }

    /// A type defined in the PyO3 module.
    ///
    /// - Types are referenced using fully qualified names to avoid symbol collision when used across modules.
    /// - For example, if `A` is defined in `package.submod1`, it will be referenced as `submod1.A` when used in other modules.
    /// - The module will be imported as `from package import submod1`.
    /// - When used in the same module where it's defined, it will be automatically de-qualified during stub generation.
    /// - The `source_module` field tracks which module the type belongs to for future use.
    ///
    /// ```
    /// pyo3_stub_gen::TypeInfo::locally_defined("A", "package.submod1".into());
    /// ```
    pub fn locally_defined(type_name: &str, module: ModuleRef) -> Self {
        let mut import = HashSet::new();
        let mut type_refs = HashMap::new();

        // Determine qualified name and import based on module
        // We qualify all named modules; de-qualification for same-module usage happens during stub generation
        let qualified_name = match module.get() {
            Some(module_name) if !module_name.is_empty() => {
                // Extract the last component of the module path for qualification
                // e.g., "package.module.submodule" -> "submodule"
                let module_component = module_name.rsplit('.').next().unwrap_or(module_name);
                // Use Module import for cross-module references
                import.insert(ImportRef::Module(module.clone()));

                // Populate type_refs with the bare identifier for context-aware qualification
                type_refs.insert(
                    type_name.to_string(),
                    TypeIdentifierRef {
                        module: module.clone(),
                        import_kind: ImportKind::Module,
                    },
                );

                format!("{}.{}", module_component, type_name)
            }
            _ => {
                // Default/empty module - treat like named modules but keep name unqualified
                // Will be resolved to actual module name at runtime
                import.insert(ImportRef::Module(module.clone()));
                type_refs.insert(
                    type_name.to_string(),
                    TypeIdentifierRef {
                        module: module.clone(),
                        import_kind: ImportKind::Module,
                    },
                );
                type_name.to_string()
            }
        };

        Self {
            name: qualified_name,
            source_module: Some(module),
            import,
            type_refs,
        }
    }

    /// Get the qualified name for use in a specific target module.
    ///
    /// - If the type has no source module, returns the name as-is
    /// - If the type is from the same module as the target, returns unqualified name
    /// - If the type is from a different module, returns qualified name with module component
    ///
    /// # Examples
    ///
    /// - Type A from "package.sub_mod" used in "package.sub_mod" -> "A"
    /// - Type A from "package.sub_mod" used in "package.main_mod" -> "sub_mod.A"
    pub fn qualified_name(&self, target_module: &str) -> String {
        match &self.source_module {
            None => self.name.clone(),
            Some(module_ref) => {
                let source = module_ref.get().unwrap_or(target_module);
                if source == target_module {
                    // Same module: unqualified
                    // Strip module prefix if present (handles pre-qualified names from macros)
                    let module_component = source.rsplit('.').next().unwrap_or(source);
                    let prefix = format!("{}.", module_component);
                    if let Some(stripped) = self.name.strip_prefix(&prefix) {
                        stripped.to_string()
                    } else {
                        self.name.clone()
                    }
                } else {
                    // Different module: qualify with last module component
                    let module_component = source.rsplit('.').next().unwrap_or(source);
                    // Strip existing module prefix if present (handles pre-qualified names from macros)
                    let prefix = format!("{}.", module_component);
                    let base_name = if let Some(stripped) = self.name.strip_prefix(&prefix) {
                        stripped
                    } else {
                        &self.name
                    };
                    format!("{}.{}", module_component, base_name)
                }
            }
        }
    }

    /// Check if this type is from the same module as the target module.
    pub fn is_same_module(&self, target_module: &str) -> bool {
        self.source_module.as_ref().and_then(|m| m.get()) == Some(target_module)
    }

    /// Check if this type is internal to the package (starts with package root).
    pub fn is_internal_to_package(&self, package_root: &str) -> bool {
        match &self.source_module {
            Some(ModuleRef::Named(path)) => path.starts_with(package_root),
            Some(ModuleRef::Default) => true,
            None => false,
        }
    }

    /// Get the qualified name for use in a specific target module with context-aware rewriting.
    ///
    /// This method handles compound type expressions by rewriting nested identifiers
    /// based on the type_refs tracking information. For example:
    /// - `typing.Optional[ClassA]` becomes `typing.Optional[sub_mod.ClassA]` when ClassA
    ///   is from a different module.
    ///
    /// # Arguments
    /// * `target_module` - The module where this type will be used
    ///
    /// # Returns
    /// The qualified type name string with identifiers properly qualified
    pub fn qualified_for_module(&self, target_module: &str) -> String {
        // If no type_refs, use the simpler qualified_name method
        if self.type_refs.is_empty() {
            return self.qualified_name(target_module);
        }

        // Rewrite the expression with context-aware qualification
        use crate::generate::qualifier::TypeExpressionQualifier;
        TypeExpressionQualifier::qualify_expression(&self.name, &self.type_refs, target_module)
    }

    /// Resolve ModuleRef::Default to the actual module name.
    /// Called at runtime when default module name is known.
    pub fn resolve_default_module(&mut self, default_module_name: &str) {
        // Resolve source_module
        if let Some(ModuleRef::Default) = &self.source_module {
            self.source_module = Some(ModuleRef::Named(default_module_name.to_string()));

            // Update qualified name if needed
            let module_component = default_module_name
                .rsplit('.')
                .next()
                .unwrap_or(default_module_name);
            if !self.name.contains('.') {
                self.name = format!("{}.{}", module_component, self.name);
            }
        }

        // Resolve import refs
        let mut new_import = std::collections::HashSet::new();
        for import_ref in &self.import {
            match import_ref {
                ImportRef::Module(ModuleRef::Default) => {
                    new_import.insert(ImportRef::Module(ModuleRef::Named(
                        default_module_name.to_string(),
                    )));
                }
                other => {
                    new_import.insert(other.clone());
                }
            }
        }
        self.import = new_import;

        // Resolve type_refs
        for type_ref in self.type_refs.values_mut() {
            if let ModuleRef::Default = &type_ref.module {
                type_ref.module = ModuleRef::Named(default_module_name.to_string());
            }
        }
    }
}

impl ops::BitOr for TypeInfo {
    type Output = Self;

    fn bitor(mut self, rhs: Self) -> Self {
        self.import.extend(rhs.import);
        // Merge type_refs from both sides
        let mut merged_type_refs = self.type_refs.clone();
        merged_type_refs.extend(rhs.type_refs);
        Self {
            name: format!("{} | {}", self.name, rhs.name),
            source_module: None, // Union types are synthetic, have no source module
            import: self.import,
            type_refs: merged_type_refs,
        }
    }
}

/// Implement [PyStubType]
///
/// ```rust
/// use pyo3::*;
/// use pyo3_stub_gen::{impl_stub_type, derive::*};
///
/// #[gen_stub_pyclass]
/// #[pyclass]
/// struct A;
///
/// #[gen_stub_pyclass]
/// #[pyclass]
/// struct B;
///
/// enum E {
///     A(A),
///     B(B),
/// }
/// impl_stub_type!(E = A | B);
///
/// struct X(A);
/// impl_stub_type!(X = A);
///
/// struct Y {
///    a: A,
///    b: B,
/// }
/// impl_stub_type!(Y = (A, B));
/// ```
#[macro_export]
macro_rules! impl_stub_type {
    ($ty: ty = $($base:ty)|+) => {
        impl ::pyo3_stub_gen::PyStubType for $ty {
            fn type_output() -> ::pyo3_stub_gen::TypeInfo {
                $(<$base>::type_output()) | *
            }
            fn type_input() -> ::pyo3_stub_gen::TypeInfo {
                $(<$base>::type_input()) | *
            }
        }
    };
    ($ty:ty = $base:ty) => {
        impl ::pyo3_stub_gen::PyStubType for $ty {
            fn type_output() -> ::pyo3_stub_gen::TypeInfo {
                <$base>::type_output()
            }
            fn type_input() -> ::pyo3_stub_gen::TypeInfo {
                <$base>::type_input()
            }
        }
    };
}

/// Annotate Rust types with Python type information.
pub trait PyStubType {
    /// The type to be used in the output signature, i.e. return type of the Python function or methods.
    fn type_output() -> TypeInfo;

    /// The type to be used in the input signature, i.e. the arguments of the Python function or methods.
    ///
    /// This defaults to the output type, but can be overridden for types that are not valid input types.
    /// For example, `Vec::<T>::type_output` returns `list[T]` while `Vec::<T>::type_input` returns `typing.Sequence[T]`.
    fn type_input() -> TypeInfo {
        Self::type_output()
    }
}

#[cfg(test)]
mod test {
    use super::*;
    use maplit::hashset;
    use std::collections::HashMap;
    use test_case::test_case;

    #[test_case(bool::type_input(), "builtins.bool", hashset! { "builtins".into() } ; "bool_input")]
    #[test_case(<&str>::type_input(), "builtins.str", hashset! { "builtins".into() } ; "str_input")]
    #[test_case(Vec::<u32>::type_input(), "typing.Sequence[builtins.int]", hashset! { "typing".into(), "builtins".into() } ; "Vec_u32_input")]
    #[test_case(Vec::<u32>::type_output(), "builtins.list[builtins.int]", hashset! {  "builtins".into() } ; "Vec_u32_output")]
    #[test_case(HashMap::<u32, String>::type_input(), "typing.Mapping[builtins.int, builtins.str]", hashset! { "typing".into(), "builtins".into() } ; "HashMap_u32_String_input")]
    #[test_case(HashMap::<u32, String>::type_output(), "builtins.dict[builtins.int, builtins.str]", hashset! { "builtins".into() } ; "HashMap_u32_String_output")]
    #[test_case(indexmap::IndexMap::<u32, String>::type_input(), "typing.Mapping[builtins.int, builtins.str]", hashset! { "typing".into(), "builtins".into() } ; "IndexMap_u32_String_input")]
    #[test_case(indexmap::IndexMap::<u32, String>::type_output(), "builtins.dict[builtins.int, builtins.str]", hashset! { "builtins".into() } ; "IndexMap_u32_String_output")]
    #[test_case(HashMap::<u32, Vec<u32>>::type_input(), "typing.Mapping[builtins.int, typing.Sequence[builtins.int]]", hashset! { "builtins".into(), "typing".into() } ; "HashMap_u32_Vec_u32_input")]
    #[test_case(HashMap::<u32, Vec<u32>>::type_output(), "builtins.dict[builtins.int, builtins.list[builtins.int]]", hashset! { "builtins".into() } ; "HashMap_u32_Vec_u32_output")]
    #[test_case(HashSet::<u32>::type_input(), "builtins.set[builtins.int]", hashset! { "builtins".into() } ; "HashSet_u32_input")]
    #[test_case(indexmap::IndexSet::<u32>::type_input(), "builtins.set[builtins.int]", hashset! { "builtins".into() } ; "IndexSet_u32_input")]
    #[test_case(TypeInfo::dict_of::<u32, String>(), "builtins.dict[builtins.int, builtins.str]", hashset! { "builtins".into() } ; "dict_of_u32_String")]
    fn test(tinfo: TypeInfo, name: &str, import: HashSet<ImportRef>) {
        assert_eq!(tinfo.name, name);
        if import.is_empty() {
            assert!(tinfo.import.is_empty());
        } else {
            assert_eq!(tinfo.import, import);
        }
    }
}
