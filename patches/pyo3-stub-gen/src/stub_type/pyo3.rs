use crate::stub_type::*;
use ::pyo3::{
    basic::CompareOp,
    pybacked::{PyBackedBytes, PyBackedStr},
    pyclass::boolean_struct::False,
    types::*,
    Bound, Py, PyClass, PyRef, PyRefMut,
};
use maplit::hashset;
use std::collections::HashMap;

impl PyStubType for PyAny {
    fn type_output() -> TypeInfo {
        TypeInfo {
            name: "typing.Any".to_string(),
            source_module: None,
            import: hashset! { "typing".into() },
            type_refs: HashMap::new(),
        }
    }
}

impl<T: PyStubType> PyStubType for Py<T> {
    fn type_input() -> TypeInfo {
        T::type_input()
    }
    fn type_output() -> TypeInfo {
        T::type_output()
    }
}

impl<T: PyStubType + PyClass> PyStubType for PyRef<'_, T> {
    fn type_input() -> TypeInfo {
        T::type_input()
    }
    fn type_output() -> TypeInfo {
        T::type_output()
    }
}

impl<T: PyStubType + PyClass<Frozen = False>> PyStubType for PyRefMut<'_, T> {
    fn type_input() -> TypeInfo {
        T::type_input()
    }
    fn type_output() -> TypeInfo {
        T::type_output()
    }
}

impl<T: PyStubType> PyStubType for Bound<'_, T> {
    fn type_input() -> TypeInfo {
        T::type_input()
    }
    fn type_output() -> TypeInfo {
        T::type_output()
    }
}

macro_rules! impl_builtin {
    ($ty:ty, $pytype:expr) => {
        impl PyStubType for $ty {
            fn type_output() -> TypeInfo {
                TypeInfo {
                    name: $pytype.to_string(),
                    source_module: None,
                    import: HashSet::new(),
                    type_refs: HashMap::new(),
                }
            }
        }
    };
}

impl_builtin!(PyBool, "bool");
impl_builtin!(PyInt, "int");
impl_builtin!(PyFloat, "float");
impl_builtin!(PyComplex, "complex");
impl_builtin!(PyList, "list");
impl_builtin!(PyTuple, "tuple");
impl_builtin!(PySlice, "slice");
impl_builtin!(PyDict, "dict");
impl_builtin!(PySet, "set");
impl_builtin!(PyString, "str");
impl_builtin!(PyBackedStr, "str");
impl_builtin!(PyByteArray, "bytearray");
impl_builtin!(PyBytes, "bytes");
impl_builtin!(PyBackedBytes, "bytes");
impl_builtin!(PyType, "type");
impl_builtin!(CompareOp, "int");

macro_rules! impl_simple {
    ($ty:ty, $mod:expr, $pytype:expr) => {
        impl PyStubType for $ty {
            fn type_output() -> TypeInfo {
                TypeInfo {
                    name: concat!($mod, ".", $pytype).to_string(),
                    source_module: None,
                    import: hashset! { $mod.into() },
                    type_refs: HashMap::new(),
                }
            }
        }
    };
}

impl_simple!(PyDate, "datetime", "date");
impl_simple!(PyDateTime, "datetime", "datetime");
impl_simple!(PyDelta, "datetime", "timedelta");
impl_simple!(PyTime, "datetime", "time");
impl_simple!(PyTzInfo, "datetime", "tzinfo");
