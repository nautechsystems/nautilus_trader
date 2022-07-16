use nautilus_model::data::bar::Bar;
use nautilus_core::string::pystr_to_string;
use pyo3::ffi;
use nautilus_model::data::tick::QuoteTick;
use pyo3::types::{PyList, PyString};
use pyo3::Python;
use pyo3::PyObject;

//////////////////////////////////////////////////////
// Data Catalog - Quote Tick C API

#[repr(C)]
pub struct Vec_QuoteTick {
    ptr: *mut QuoteTick,
    len: usize,
    cap: usize
}

#[no_mangle]
pub extern "C" fn index_quote_tick_vector(ptr: &Vec<QuoteTick>, i: usize)
-> &QuoteTick { &ptr[i] }


#[no_mangle]
pub unsafe extern "C" fn read_parquet_ticks(
    path: *mut ffi::PyObject,
    filter_exprs: *mut ffi::PyObject
) -> Vec<QuoteTick> {
    let path = pystr_to_string(path);
    let filter_exprs = _extract_filter_exprs(filter_exprs);
    _read_parquet_ticks(path, filter_exprs)
}

fn _read_parquet_ticks(
    path: String,
    filter_exprs: Option<Vec<String>>,
) -> Vec<QuoteTick> {
    todo!()    
}

//////////////////////////////////////////////////////
// Data Catalog - Bar C API

#[repr(C)]
pub struct Vec_Bar {
    ptr: *mut Bar,
    len: usize,
    cap: usize
}

#[no_mangle]
pub extern "C" fn index_bar_vector(ptr: &Vec<Bar>, i: usize)
-> &Bar { &ptr[i] }

#[no_mangle]
pub unsafe extern "C" fn read_parquet_bars(
    path: *mut ffi::PyObject,
    filter_exprs: *mut ffi::PyObject
) 
-> Vec<Bar> {
    let path = pystr_to_string(path);
    let filter_exprs = _extract_filter_exprs(filter_exprs);
    _read_parquet_bars(path, filter_exprs)
}

fn _read_parquet_bars(
    path: String,
    filter_exprs: Option<Vec<String>>,
) -> Vec<Bar> {
    todo!()
}

//////////////////////////////////////////////////////

fn _extract_filter_exprs(filter_exprs: *mut ffi::PyObject) 
-> Option<Vec<String>> {
    unsafe{
        Python::with_gil(|py| 
            PyObject::from_borrowed_ptr(py, filter_exprs)
            .extract::<Option<Vec<String>>>(py).unwrap()
        )
    }
}

//////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::{prepare_freethreaded_python, IntoPyPointer};
    use pyo3::ffi::Py_None;
    
    #[test]
    fn test_read_parquet_ticks() {
        prepare_freethreaded_python();
        let gil = Python::acquire_gil();
        let py = gil.python();
        let path = PyString::new(py, "some_path").into_ptr();
        let filter_exprs = PyList::new(py, vec!["filter_expr1", "filter_expr2", "filter_expr3"]).into_ptr();
        // let filter_exprs = Py_None();
        unsafe {
            read_parquet_ticks(path, filter_exprs);
        }
        todo!();
    }

}
