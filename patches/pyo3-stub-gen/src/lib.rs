//! This crate creates stub files in following three steps using [inventory] crate:
//!
//! Define type information in Rust code (or by proc-macro)
//! ---------------------------------------------------------
//! The first step is to define Python type information in Rust code. [type_info] module provides several structs, for example:
//!
//! - [type_info::PyFunctionInfo] stores information of Python function, i.e. the name of the function, arguments and its types, return type, etc.
//! - [type_info::PyClassInfo] stores information for Python class definition, i.e. the name of the class, members and its types, methods, etc.
//!
//! For better understanding of what happens in the background, let's define these information manually:
//!
//! ```
//! use pyo3::*;
//! use pyo3_stub_gen::type_info::*;
//!
//! // Usual PyO3 class definition
//! #[pyclass(module = "my_module", name = "MyClass")]
//! struct MyClass {
//!     #[pyo3(get)]
//!     name: String,
//!     #[pyo3(get)]
//!     description: Option<String>,
//! }
//!
//! // Submit type information for stub file generation to inventory
//! inventory::submit!{
//!     // Send information about Python class
//!     PyClassInfo {
//!         // Type ID of Rust struct (used to gathering phase discussed later)
//!         struct_id: std::any::TypeId::of::<MyClass>,
//!
//!         // Python module name. Since stub file is generated per modules,
//!         // this helps where the class definition should be placed.
//!         module: Some("my_module"),
//!
//!         // Python class name
//!         pyclass_name: "MyClass",
//!
//!         getters: &[
//!             MemberInfo {
//!                 name: "name",
//!                 r#type: <String as ::pyo3_stub_gen::PyStubType>::type_output,
//!                 doc: "Name docstring",
//!                 default: None,
//!                 deprecated: None,
//!             },
//!             MemberInfo {
//!                 name: "description",
//!                 r#type: <Option<String> as ::pyo3_stub_gen::PyStubType>::type_output,
//!                 doc: "Description docstring",
//!                 default: None,
//!                 deprecated: None,
//!             },
//!         ],
//!
//!         setters: &[],
//!
//!         doc: "Docstring used in Python",
//!
//!         // Base classes
//!         bases: &[],
//!
//!         // Decorated with `#[pyclass(eq, ord)]`
//!         has_eq: false,
//!         has_ord: false,
//!         // Decorated with `#[pyclass(hash, str)]`
//!         has_hash: false,
//!         has_str: false,
//!         // Decorated with `#[pyclass(subclass)]`
//!         subclass: false,
//!     }
//! }
//! ```
//!
//! Roughly speaking, the above corresponds a following stub file `my_module.pyi`:
//!
//! ```python
//! class MyClass:
//!     """
//!     Docstring used in Python
//!     """
//!     name: str
//!     """Name docstring"""
//!     description: Optional[str]
//!     """Description docstring"""
//! ```
//!
//! We want to generate this [type_info::PyClassInfo] section automatically from `MyClass` Rust struct definition.
//! This is done by using `#[gen_stub_pyclass]` proc-macro:
//!
//! ```
//! use pyo3::*;
//! use pyo3_stub_gen::{type_info::*, derive::gen_stub_pyclass};
//!
//! // Usual PyO3 class definition
//! #[gen_stub_pyclass]
//! #[pyclass(module = "my_module", name = "MyClass")]
//! struct MyClass {
//!     #[pyo3(get)]
//!     name: String,
//!     #[pyo3(get)]
//!     description: Option<String>,
//! }
//! ```
//!
//! Since proc-macro is a converter from Rust code to Rust code, the output must be a Rust code.
//! However, we need to gather these [type_info::PyClassInfo] definitions to generate stub files,
//! and the above [inventory::submit] is for it.
//!
//! Gather type information into [StubInfo]
//! ----------------------------------------
//! [inventory] crate provides a mechanism to gather [inventory::submit]ted information when the library is loaded.
//! To access these information through [inventory::iter], we need to define a gather function in the crate.
//! Typically, this is done by following:
//!
//! ```rust
//! use pyo3_stub_gen::{StubInfo, Result};
//!
//! pub fn stub_info() -> Result<StubInfo> {
//!     let manifest_dir: &::std::path::Path = env!("CARGO_MANIFEST_DIR").as_ref();
//!     StubInfo::from_pyproject_toml(manifest_dir.join("pyproject.toml"))
//! }
//! ```
//!
//! There is a helper macro to define it easily:
//!
//! ```rust
//! pyo3_stub_gen::define_stub_info_gatherer!(sub_info);
//! ```
//!
//! Generate stub file from [StubInfo]
//! -----------------------------------
//! [StubInfo] translates [type_info::PyClassInfo] and other information into a form helpful for generating stub files while gathering.
//!
//! [generate] module provides structs implementing [std::fmt::Display] to generate corresponding parts of stub file.
//! For example, [generate::MethodDef] generates Python class method definition as follows:
//!
//! ```rust
//! use pyo3_stub_gen::{TypeInfo, generate::*, type_info::ParameterKind};
//!
//! let method = MethodDef {
//!     name: "foo",
//!     parameters: Parameters {
//!         positional_or_keyword: vec![Parameter {
//!             name: "x",
//!             kind: ParameterKind::PositionalOrKeyword,
//!             type_info: TypeInfo::builtin("int"),
//!             default: ParameterDefault::None,
//!         }],
//!         ..Parameters::new()
//!     },
//!     r#return: TypeInfo::builtin("int"),
//!     doc: "This is a foo method.",
//!     r#type: MethodType::Instance,
//!     deprecated: None,
//!     is_async: false,
//!     type_ignored: None,
//!     is_overload: false,
//! };
//!
//! assert_eq!(
//!     method.to_string().trim(),
//!     r#"
//!     def foo(self, x: builtins.int) -> builtins.int:
//!         r"""
//!         This is a foo method.
//!         """
//!     "#.trim()
//! );
//! ```
//!
//! [generate::ClassDef] generates Python class definition using [generate::MethodDef] and others, and other `*Def` structs works as well.
//!
//! [generate::Module] consists of `*Def` structs and yields an entire stub file `*.pyi` for a single Python (sub-)module, i.e. a shared library build by PyO3.
//! [generate::Module]s are created as a part of [StubInfo], which merges [type_info::PyClassInfo]s and others submitted to [inventory] separately.
//! [StubInfo] is instantiated with [pyproject::PyProject] to get where to generate the stub file,
//! and [StubInfo::generate] generates the stub files for every modules.
//!

pub use inventory;
pub use pyo3_stub_gen_derive as derive; // re-export to use in generated code

pub mod docgen;
pub mod exception;
pub mod generate;
pub mod pyproject;
pub mod rule_name;
mod stub_type;
pub mod type_info;
pub mod util;

pub use generate::StubInfo;
pub use pyproject::StubGenConfig;
pub use stub_type::{ImportKind, ImportRef, ModuleRef, PyStubType, TypeIdentifierRef, TypeInfo};

pub type Result<T> = anyhow::Result<T>;

/// Create a function to initialize [StubInfo] from `pyproject.toml` in `CARGO_MANIFEST_DIR`.
///
/// If `pyproject.toml` is in another place, you need to create a function to call [StubInfo::from_pyproject_toml] manually.
/// This must be placed in your PyO3 library crate, i.e. same crate where [inventory::submit]ted,
/// not in `gen_stub` executables due to [inventory] mechanism.
///
#[macro_export]
macro_rules! define_stub_info_gatherer {
    ($function_name:ident) => {
        /// Auto-generated function to gather information to generate stub files
        pub fn $function_name() -> $crate::Result<$crate::StubInfo> {
            let manifest_dir: &::std::path::Path = env!("CARGO_MANIFEST_DIR").as_ref();
            $crate::StubInfo::from_pyproject_toml(manifest_dir.join("pyproject.toml"))
        }
    };
}

/// Add module-level documention using interpolation of runtime expressions.
/// The first argument `module_doc!` receives is the full module name;
/// the second and followings are a format string, same to `format!`.
/// ```rust
/// pyo3_stub_gen::module_doc!(
///   "module.name",
///   "Document for {} v{} ...",
///   env!("CARGO_PKG_NAME"),
///   env!("CARGO_PKG_VERSION")
/// );
/// ```
#[macro_export]
macro_rules! module_doc {
    ($module:literal, $($fmt:tt)+) => {
        $crate::inventory::submit! {
            $crate::type_info::ModuleDocInfo {
                module: $module,
                doc: {
                    fn _fmt() -> String {
                        ::std::format!($($fmt)+)
                    }
                    _fmt
                }
            }
        }
    };
}

/// Add module-level variable, the first argument `module_variable!` receives is the full module name;
/// the second argument is the name of the variable, the third argument is the type of the variable,
/// and (optional) the fourth argument is the default value of the variable.
/// ```rust
/// pyo3_stub_gen::module_variable!("module.name", "CONSTANT1", usize);
/// pyo3_stub_gen::module_variable!("module.name", "CONSTANT2", usize, 123);
/// ```
#[macro_export]
macro_rules! module_variable {
    ($module:expr, $name:expr, $ty:ty) => {
        $crate::inventory::submit! {
            $crate::type_info::PyVariableInfo{
                name: $name,
                module: $module,
                r#type: <$ty as $crate::PyStubType>::type_output,
                default: None,
            }
        }
    };
    ($module:expr, $name:expr, $ty:ty, $value:expr) => {
        $crate::inventory::submit! {
            $crate::type_info::PyVariableInfo{
                name: $name,
                module: $module,
                r#type: <$ty as $crate::PyStubType>::type_output,
                default: Some({
                    fn _fmt() -> String {
                        let v: $ty = $value;
                        $crate::util::fmt_py_obj(v)
                    }
                    _fmt
                }),
            }
        }
    };
}

/// Add module-level type alias using TypeInfo
///
/// This macro supports both single types and union types.
///
/// # Examples
///
/// Single type:
/// ```rust
/// pyo3_stub_gen::type_alias!("module.name", MyAlias = Option<usize>);
/// ```
///
/// Union type (direct syntax):
/// ```rust
/// pyo3_stub_gen::type_alias!("module.name", MyUnion = i32 | String);
/// ```
/// ```rust,ignore
/// pyo3_stub_gen::type_alias!("module.name", StructUnion = Bound<'static, TypeA> | Bound<'static, TypeB>);
/// ```
#[macro_export]
macro_rules! type_alias {
    // Pattern 1: Union types with docstring - must come first
    ($module:expr, $name:ident = $($base:ty)|+, $doc:expr) => {
        const _: () = {
            struct __TypeAliasImpl;

            impl $crate::PyStubType for __TypeAliasImpl {
                fn type_output() -> $crate::TypeInfo {
                    $(<$base>::type_output()) | *
                }
                fn type_input() -> $crate::TypeInfo {
                    $(<$base>::type_input()) | *
                }
            }

            $crate::inventory::submit! {
                $crate::type_info::TypeAliasInfo {
                    name: stringify!($name),
                    module: $module,
                    r#type: <__TypeAliasImpl as $crate::PyStubType>::type_output,
                    doc: $doc,
                }
            }
        };
    };

    // Pattern 2: Union types without docstring (backward compatible)
    ($module:expr, $name:ident = $($base:ty)|+) => {
        const _: () = {
            struct __TypeAliasImpl;

            impl $crate::PyStubType for __TypeAliasImpl {
                fn type_output() -> $crate::TypeInfo {
                    $(<$base>::type_output()) | *
                }
                fn type_input() -> $crate::TypeInfo {
                    $(<$base>::type_input()) | *
                }
            }

            $crate::inventory::submit! {
                $crate::type_info::TypeAliasInfo {
                    name: stringify!($name),
                    module: $module,
                    r#type: <__TypeAliasImpl as $crate::PyStubType>::type_output,
                    doc: "",
                }
            }
        };
    };

    // Pattern 3: Single types with docstring
    ($module:expr, $name:ident = $ty:ty, $doc:expr) => {
        $crate::inventory::submit! {
            $crate::type_info::TypeAliasInfo {
                name: stringify!($name),
                module: $module,
                r#type: <$ty as $crate::PyStubType>::type_output,
                doc: $doc,
            }
        }
    };

    // Pattern 4: Single types without docstring (backward compatible)
    ($module:expr, $name:ident = $ty:ty) => {
        $crate::inventory::submit! {
            $crate::type_info::TypeAliasInfo {
                name: stringify!($name),
                module: $module,
                r#type: <$ty as $crate::PyStubType>::type_output,
                doc: "",
            }
        }
    };
}

/// Re-export items from another module into __all__
///
/// # Wildcard re-export
/// ```rust
/// pyo3_stub_gen::reexport_module_members!("target.module", "source.module");
/// ```
///
/// # Specific items re-export
/// ```rust
/// pyo3_stub_gen::reexport_module_members!("target.module", "source.module", "item1", "item2");
/// ```
#[macro_export]
macro_rules! reexport_module_members {
    // Wildcard: reexport_module_members!("target", "source")
    ($target:expr, $source:expr) => {
        $crate::inventory::submit! {
            $crate::type_info::ReexportModuleMembers {
                target_module: $target,
                source_module: $source,
                items: None,
            }
        }
    };
    // Specific items: reexport_module_members!("target", "source", "item1", "item2")
    ($target:expr, $source:expr, $($item:expr),+) => {
        $crate::inventory::submit! {
            $crate::type_info::ReexportModuleMembers {
                target_module: $target,
                source_module: $source,
                items: Some(&[$($item),+]),
            }
        }
    };
}

/// Add verbatim entry to __all__
///
/// # Example
/// ```rust
/// pyo3_stub_gen::export_verbatim!("my.module", "my_name");
/// ```
#[macro_export]
macro_rules! export_verbatim {
    ($module:expr, $name:expr) => {
        $crate::inventory::submit! {
            $crate::type_info::ExportVerbatim {
                target_module: $module,
                name: $name,
            }
        }
    };
}

/// Exclude specific items from __all__
///
/// # Example
/// ```rust
/// pyo3_stub_gen::exclude_from_all!("my.module", "internal_function");
/// ```
#[macro_export]
macro_rules! exclude_from_all {
    ($module:expr, $name:expr) => {
        $crate::inventory::submit! {
            $crate::type_info::ExcludeFromAll {
                target_module: $module,
                name: $name,
            }
        }
    };
}

#[doc = include_str!("../README.md")]
mod readme {}
