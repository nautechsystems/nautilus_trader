//! Store of metadata for generating Python stub file
//!
//! Stub file generation takes two steps:
//!
//! Store metadata (compile time)
//! ------------------------------
//! Embed compile-time information about Rust types and PyO3 macro arguments
//! using [inventory::submit!](https://docs.rs/inventory/latest/inventory/macro.submit.html) macro into source codes,
//! and these information will be gathered by [inventory::iter](https://docs.rs/inventory/latest/inventory/struct.iter.html).
//! This submodule is responsible for this process.
//!
//! - [PyClassInfo] stores information obtained from `#[pyclass]` macro
//! - [PyMethodsInfo] stores information obtained from `#[pymethods]` macro
//!
//! and others are their components.
//!
//! Gathering metadata and generating stub file (runtime)
//! -------------------------------------------------------
//! Since `#[pyclass]` and `#[pymethods]` definitions are not bundled in a single block,
//! we have to reconstruct these information corresponding to a Python `class`.
//! This process is done at runtime in [gen_stub](../../gen_stub) executable.
//!

use crate::{PyStubType, TypeInfo};
use std::any::TypeId;

/// Represents the target of type ignore comments
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum IgnoreTarget {
    /// Ignore all type checking errors `(# type: ignore)`
    All,
    /// Ignore specific type checking rules `(# type: ignore[rule1,rule2])`
    Specified(&'static [&'static str]),
}

/// Information about deprecated items
#[derive(Debug, Clone, PartialEq)]
pub struct DeprecatedInfo {
    pub since: Option<&'static str>,
    pub note: Option<&'static str>,
}

/// Work around for `CompareOp` for `__richcmp__` argument,
/// which does not implements `FromPyObject`
pub fn compare_op_type_input() -> TypeInfo {
    <isize as PyStubType>::type_input()
}

pub fn no_return_type_output() -> TypeInfo {
    TypeInfo::none()
}

/// Kind of parameter in Python function signature
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParameterKind {
    /// Positional-only parameter (before `/`)
    PositionalOnly,
    /// Positional or keyword parameter (default)
    PositionalOrKeyword,
    /// Keyword-only parameter (after `*`)
    KeywordOnly,
    /// Variable positional parameter (`*args`)
    VarPositional,
    /// Variable keyword parameter (`**kwargs`)
    VarKeyword,
}

/// Default value of a parameter
#[derive(Debug, Clone)]
pub enum ParameterDefault {
    /// No default value
    None,
    /// Default value expression as a string
    Expr(fn() -> String),
}

impl PartialEq for ParameterDefault {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Expr(l), Self::Expr(r)) => {
                let l_val = l();
                let r_val = r();
                l_val.eq(&r_val)
            }
            (Self::None, Self::None) => true,
            _ => false,
        }
    }
}

/// Information about a parameter in a Python function/method signature
///
/// This struct is used at compile time to store metadata about parameters
/// that will be used to generate Python stub files.
#[derive(Debug)]
pub struct ParameterInfo {
    /// Parameter name
    pub name: &'static str,
    /// Parameter kind (positional-only, keyword-only, etc.)
    pub kind: ParameterKind,
    /// Type information getter
    pub type_info: fn() -> TypeInfo,
    /// Default value
    pub default: ParameterDefault,
}

/// Type of a method
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MethodType {
    Instance,
    Static,
    Class,
    New,
}

/// Info of usual method appears in `#[pymethod]`
#[derive(Debug)]
pub struct MethodInfo {
    pub name: &'static str,
    pub parameters: &'static [ParameterInfo],
    pub r#return: fn() -> TypeInfo,
    pub doc: &'static str,
    pub r#type: MethodType,
    pub is_async: bool,
    pub deprecated: Option<DeprecatedInfo>,
    pub type_ignored: Option<IgnoreTarget>,
    /// Whether this method is marked as an overload variant
    pub is_overload: bool,
}

/// Info of getter method decorated with `#[getter]` or `#[pyo3(get, set)]` appears in `#[pyclass]`
#[derive(Debug)]
pub struct MemberInfo {
    pub name: &'static str,
    pub r#type: fn() -> TypeInfo,
    pub doc: &'static str,
    pub default: Option<fn() -> String>,
    pub deprecated: Option<DeprecatedInfo>,
}

/// Info of `#[pymethod]`
#[derive(Debug)]
pub struct PyMethodsInfo {
    // The Rust struct type-id of `impl` block where `#[pymethod]` acts on
    pub struct_id: fn() -> TypeId,
    /// Method/Const with `#[classattr]`
    pub attrs: &'static [MemberInfo],
    /// Methods decorated with `#[getter]`
    pub getters: &'static [MemberInfo],
    /// Methods decorated with `#[getter]`
    pub setters: &'static [MemberInfo],
    /// Other usual methods
    pub methods: &'static [MethodInfo],
    /// Source file location for deterministic ordering
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
}

inventory::collect!(PyMethodsInfo);

/// Info of `#[pyclass]` with Rust struct
#[derive(Debug)]
pub struct PyClassInfo {
    // Rust struct type-id
    pub struct_id: fn() -> TypeId,
    // The name exposed to Python
    pub pyclass_name: &'static str,
    /// Module name specified by `#[pyclass(module = "foo.bar")]`
    pub module: Option<&'static str>,
    /// Docstring
    pub doc: &'static str,
    /// static members by `#[pyo3(get)]`
    pub getters: &'static [MemberInfo],
    /// static members by `#[pyo3(set)]`
    pub setters: &'static [MemberInfo],
    /// Base classes specified by `#[pyclass(extends = Type)]`
    pub bases: &'static [fn() -> TypeInfo],
    /// Whether the class has eq attribute
    pub has_eq: bool,
    /// Whether the class has ord attribute
    pub has_ord: bool,
    /// Whether the class has hash attribute
    pub has_hash: bool,
    /// Whether the class has str attribute
    pub has_str: bool,
    /// Whether the class has subclass attribute `#[pyclass(subclass)]`
    pub subclass: bool,
}

inventory::collect!(PyClassInfo);

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum VariantForm {
    Unit,
    Tuple,
    Struct,
}

/// Info of a `#[pyclass]` with a single variant of a rich (structured) Rust enum
#[derive(Debug)]
pub struct VariantInfo {
    pub pyclass_name: &'static str,
    pub module: Option<&'static str>,
    pub doc: &'static str,
    pub fields: &'static [MemberInfo],
    pub form: &'static VariantForm,
    pub constr_args: &'static [ParameterInfo],
}

/// Info of a `#[pyclass]` with a rich (structured) Rust enum
#[derive(Debug)]
pub struct PyComplexEnumInfo {
    // Rust struct type-id
    pub enum_id: fn() -> TypeId,
    // The name exposed to Python
    pub pyclass_name: &'static str,
    /// Module name specified by `#[pyclass(module = "foo.bar")]`
    pub module: Option<&'static str>,
    /// Docstring
    pub doc: &'static str,
    /// static members by `#[pyo3(get, set)]`
    pub variants: &'static [VariantInfo],
}

inventory::collect!(PyComplexEnumInfo);

/// Info of `#[pyclass]` with Rust enum
#[derive(Debug)]
pub struct PyEnumInfo {
    // Rust struct type-id
    pub enum_id: fn() -> TypeId,
    // The name exposed to Python
    pub pyclass_name: &'static str,
    /// Module name specified by `#[pyclass(module = "foo.bar")]`
    pub module: Option<&'static str>,
    /// Docstring
    pub doc: &'static str,
    /// Variants of enum (name, doc)
    pub variants: &'static [(&'static str, &'static str)],
}

inventory::collect!(PyEnumInfo);

/// Info of `#[pyfunction]`
#[derive(Debug)]
pub struct PyFunctionInfo {
    pub name: &'static str,
    pub parameters: &'static [ParameterInfo],
    pub r#return: fn() -> TypeInfo,
    pub doc: &'static str,
    pub module: Option<&'static str>,
    pub is_async: bool,
    pub deprecated: Option<DeprecatedInfo>,
    pub type_ignored: Option<IgnoreTarget>,
    /// Whether this function is marked as an overload variant
    pub is_overload: bool,
    /// Source file location for deterministic ordering
    pub file: &'static str,
    pub line: u32,
    pub column: u32,
    /// Index for ordering multiple functions from the same macro invocation
    pub index: usize,
}

inventory::collect!(PyFunctionInfo);

#[derive(Debug)]
pub struct PyVariableInfo {
    pub name: &'static str,
    pub module: &'static str,
    pub r#type: fn() -> TypeInfo,
    pub default: Option<fn() -> String>,
}

inventory::collect!(PyVariableInfo);

#[derive(Debug)]
pub struct TypeAliasInfo {
    pub name: &'static str,
    pub module: &'static str,
    pub r#type: fn() -> TypeInfo,
    pub doc: &'static str,
}

inventory::collect!(TypeAliasInfo);

#[derive(Debug)]
pub struct ModuleDocInfo {
    pub module: &'static str,
    pub doc: fn() -> String,
}

inventory::collect!(ModuleDocInfo);

/// Re-export items from another module into __all__
#[derive(Debug)]
pub struct ReexportModuleMembers {
    pub target_module: &'static str,
    pub source_module: &'static str,
    pub items: Option<&'static [&'static str]>,
}

inventory::collect!(ReexportModuleMembers);

/// Add verbatim entry to __all__
#[derive(Debug)]
pub struct ExportVerbatim {
    pub target_module: &'static str,
    pub name: &'static str,
}

inventory::collect!(ExportVerbatim);

/// Exclude specific items from __all__
#[derive(Debug)]
pub struct ExcludeFromAll {
    pub target_module: &'static str,
    pub name: &'static str,
}

inventory::collect!(ExcludeFromAll);
