use pyo3::{prelude::*, types::*};
use std::{borrow::Cow, ffi::CString};

pub fn all_builtin_types(any: &Bound<'_, PyAny>) -> bool {
    if any.is_instance_of::<PyString>()
        || any.is_instance_of::<PyBool>()
        || any.is_instance_of::<PyInt>()
        || any.is_instance_of::<PyFloat>()
        || any.is_instance_of::<PyComplex>()
        || any.is_none()
    {
        return true;
    }
    if any.is_instance_of::<PyDict>() {
        return any
            .cast::<PyDict>()
            .map(|dict| {
                dict.into_iter()
                    .all(|(k, v)| all_builtin_types(&k) && all_builtin_types(&v))
            })
            .unwrap_or(false);
    }
    if any.is_instance_of::<PyList>() {
        return any
            .cast::<PyList>()
            .map(|list| list.into_iter().all(|v| all_builtin_types(&v)))
            .unwrap_or(false);
    }
    if any.is_instance_of::<PyTuple>() {
        return any
            .cast::<PyTuple>()
            .map(|list| list.into_iter().all(|v| all_builtin_types(&v)))
            .unwrap_or(false);
    }
    false
}

/// whether eval(repr(any)) == any
pub fn valid_external_repr(any: &Bound<'_, PyAny>) -> Option<bool> {
    let globals = get_globals(any).ok()?;
    let fmt_str = any.repr().ok()?.to_string();
    let fmt_cstr = CString::new(fmt_str.clone()).ok()?;
    let new_any = any.py().eval(&fmt_cstr, Some(&globals), None).ok()?;
    new_any.eq(any).ok()
}

fn get_globals<'py>(any: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyDict>> {
    let type_object = any.get_type();
    let type_name = type_object.getattr("__name__")?;
    let type_name: Cow<str> = type_name.extract()?;
    let globals = PyDict::new(any.py());
    globals.set_item(type_name, type_object)?;
    Ok(globals)
}

#[cfg_attr(not(feature = "infer_signature"), allow(unused_variables))]
pub fn fmt_py_obj<T: for<'py> pyo3::IntoPyObjectExt<'py>>(obj: T) -> String {
    #[cfg(feature = "infer_signature")]
    {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| -> String {
            if let Ok(any) = obj.into_bound_py_any(py) {
                if all_builtin_types(&any) || valid_external_repr(&any).is_some_and(|valid| valid) {
                    if let Ok(py_str) = any.repr() {
                        return py_str.to_string();
                    }
                }
            }
            "...".to_owned()
        })
    }
    #[cfg(not(feature = "infer_signature"))]
    {
        "...".to_owned()
    }
}

#[cfg(all(test, feature = "infer_signature"))]
mod test {
    use super::*;
    #[pyclass]
    #[derive(Debug)]
    struct A {}
    #[test]
    fn test_fmt_dict() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let dict = PyDict::new(py);
            _ = dict.set_item("k1", "v1");
            _ = dict.set_item("k2", 2);
            assert_eq!("{'k1': 'v1', 'k2': 2}", fmt_py_obj(dict.as_unbound()));
            // class A variable can not be formatted
            _ = dict.set_item("k3", A {});
            assert_eq!("...", fmt_py_obj(dict.as_unbound()));
        })
    }
    #[test]
    fn test_fmt_list() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let list = PyList::new(py, [1, 2]).unwrap();
            assert_eq!("[1, 2]", fmt_py_obj(list.as_unbound()));
            // class A variable can not be formatted
            let list = PyList::new(py, [A {}, A {}]).unwrap();
            assert_eq!("...", fmt_py_obj(list.as_unbound()));
        })
    }
    #[test]
    fn test_fmt_tuple() {
        pyo3::Python::initialize();
        pyo3::Python::attach(|py| {
            let tuple = PyTuple::new(py, [1, 2]).unwrap();
            assert_eq!("(1, 2)", fmt_py_obj(tuple.as_unbound()));
            let tuple = PyTuple::new(py, [1]).unwrap();
            assert_eq!("(1,)", fmt_py_obj(tuple.as_unbound()));
            // class A variable can not be formatted
            let tuple = PyTuple::new(py, [A {}]).unwrap();
            assert_eq!("...", fmt_py_obj(tuple.as_unbound()));
        })
    }
    #[test]
    fn test_fmt_other() {
        // str
        assert_eq!("'123'", fmt_py_obj("123"));
        assert_eq!("\"don't\"", fmt_py_obj("don't"));
        assert_eq!("'str\\\\'", fmt_py_obj("str\\"));
        // bool
        assert_eq!("True", fmt_py_obj(true));
        assert_eq!("False", fmt_py_obj(false));
        // int
        assert_eq!("123", fmt_py_obj(123));
        // float
        assert_eq!("1.23", fmt_py_obj(1.23));
        // None
        let none: Option<usize> = None;
        assert_eq!("None", fmt_py_obj(none));
        // class A variable can not be formatted
        assert_eq!("...", fmt_py_obj(A {}));
    }
    #[test]
    fn test_fmt_enum() {
        #[pyclass(eq, eq_int)]
        #[derive(Debug, Clone, PartialEq, Eq, Hash)]
        pub enum Number {
            Float,
            Integer,
        }
        assert_eq!("Number.Float", fmt_py_obj(Number::Float));
    }
}
