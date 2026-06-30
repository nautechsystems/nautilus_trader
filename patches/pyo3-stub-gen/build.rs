fn main() {
    println!("cargo::rustc-check-cfg=cfg(Py_3_10)");
    pyo3_build_config::use_pyo3_cfgs();
}
