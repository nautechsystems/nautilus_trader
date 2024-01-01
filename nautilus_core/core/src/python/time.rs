use pyo3::pymethods;

use crate::time::AtomicTime;

#[pymethods]
impl AtomicTime {
    #[new]
    fn py_new(live: bool, time: u64) -> Self {
        Self::new(live, time)
    }

    #[pyo3(name = "make_live")]
    pub fn py_make_live(&self) {
        self.make_live()
    }

    #[pyo3(name = "make_static")]
    pub fn py_make_static(&self) {
        self.make_static()
    }
}
