use crate::identifiers::instrument_id::InstrumentId;
use pyo3::{ffi, FromPyPointer, Python, PyObject};
use crate::types::price::Price;
use pyo3::prelude::*;

fn type_of<T>(_: &T) -> &'static str {
    std::any::type_name::<T>()
}

/////////////////////////////////////////////
/// Parse InstrumentId or Option<InstrumentId> from a PyObject
use pyo3::prelude::PyResult;
use pyo3::{
    // Python, 
    FromPyObject,
    AsPyPointer
};
use crate::identifiers::instrument_id::instrument_id_from_pystrs;
use pyo3::types::{PyTuple, PyAny};

impl FromPyObject<'_> for InstrumentId {
    fn extract(obj: &PyAny) -> PyResult<Self> {
        let gil = Python::acquire_gil();
        let py = gil.python();
        let state = obj.call_method0("__getstate__").unwrap();
        let tupl: &PyTuple = state
                    .extract()
                    .unwrap();
        let instrument_id;
        unsafe {
            instrument_id = instrument_id_from_pystrs(
                tupl.get_item(0).unwrap().as_ptr(),
                tupl.get_item(1).unwrap().as_ptr()
            );
        }
        Ok(instrument_id)
    }
}

/////////////////////////////////////////////
#[repr(C)]
pub enum OptionTag {
    None = 0,
    Some = 1
}

#[repr(C)]
pub struct Option_InstrumentId {
    tag: OptionTag,
    some: InstrumentId
}

#[repr(C)]
pub struct Option_Price {
    tag: OptionTag,
    some: Price
}

#[no_mangle]
pub unsafe extern "C" fn pyobject_to_option_parse_test(
    instrument_id_ptr: *mut ffi::PyObject,
)
{
    let gil = Python::acquire_gil();
    let py = gil.python();
    
    // .extract method: FromPyObject trait is implemented on InstrumentId
    let instrument_id_option = 
        PyObject::from_borrowed_ptr(py, instrument_id_ptr)
        .extract::<Option<InstrumentId>>(py).unwrap();
    
    println!("{} | {:?} | {:?}", "instrument_id_option", instrument_id_option, type_of(&instrument_id_option));
}


#[no_mangle]
pub unsafe extern "C" fn create_instrument_id_option(
) -> Option<InstrumentId>
{
    Some(InstrumentId::from("EUR/USD.DUKA"))
}

#[no_mangle]
pub unsafe extern "C" fn create_price_option(
) -> Option<Price>
{
    Some(Price::from("1.2345"))
}
