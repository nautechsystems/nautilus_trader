use std::any::Any;

use pyo3::prelude::*;
use ustr::Ustr;

use crate::msgbus::MessageHandler;

#[pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common.MessageBusHandler")]
pub struct PythonMessageBusHandler {
    id: Ustr,
    handler: PyObject,
}

#[pymethods]
impl PythonMessageBusHandler {
    #[new]
    pub fn new(id: &str, handler: PyObject) -> Self {
        let id = Ustr::from(id);
        PythonMessageBusHandler { id, handler }
    }
}

impl MessageHandler for PythonMessageBusHandler {
    #[allow(unused_variables)]
    fn handle(&self, message: &dyn Any) {
        // TODO: convert message to PyObject
        let py_event = ();
        let result =
            pyo3::Python::with_gil(|py| self.handler.call_method1(py, "handle", (py_event,)));
        if let Err(err) = result {
            eprintln!("Error calling handle method: {:?}", err);
        }
    }

    fn id(&self) -> Ustr {
        self.id
    }
}
