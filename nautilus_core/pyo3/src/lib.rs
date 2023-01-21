use nautilus_persistence::persistence;
use pyo3::{prelude::*, types::PyDict};

/// Need to modify sys modules so that submodule can be loaded directly as
/// import supermodule.submodule
///
/// refer: https://github.com/PyO3/pyo3/issues/2644
#[pymodule]
fn nautilus_pyo3(py: Python<'_>, m: &PyModule) -> PyResult<()> {
    let submodule = pyo3::wrap_pymodule!(persistence);
    m.add_wrapped(submodule)?;
    let sys = PyModule::import(py, "sys")?;
    let sys_modules: &PyDict = sys.getattr("modules")?.downcast()?;
    sys_modules.set_item("nautilus_pyo3.persistence", m.getattr("persistence")?)?;
    Ok(())
}
