use pyo3::pymethods;

use crate::msgbus::BusMessage;

#[pymethods]
impl BusMessage {
    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&mut self) -> String {
        self.topic.to_string()
    }

    #[getter]
    #[pyo3(name = "payload")]
    fn py_payload(&mut self) -> &[u8] {
        self.payload.as_ref()
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(BusMessage), self)
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}
