//! Generate Python typing stub file a.k.a. `*.pyi` file.

mod class;
mod deprecated;
pub(crate) mod docstring;
mod enum_;
mod function;
mod member;
mod method;
mod module;
mod parameters;
pub(crate) mod qualifier;
mod stub_info;
mod type_alias;
mod variable;
mod variant_methods;

pub use class::*;
pub use docstring::normalize_docstring;
pub use enum_::*;
pub use function::*;
pub use member::*;
pub use method::*;
pub use module::*;
pub use parameters::*;
pub use stub_info::*;
pub use type_alias::*;
pub use variable::*;

use crate::stub_type::ImportRef;
use std::collections::HashSet;

fn indent() -> &'static str {
    "    "
}

pub trait Import {
    fn import(&self) -> HashSet<ImportRef>;
}
