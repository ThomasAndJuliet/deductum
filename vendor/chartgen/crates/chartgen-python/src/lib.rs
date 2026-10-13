//! Native Python binding; all decisions and layouts execute in Rust.
use pyo3::prelude::*;
#[pyfunction]
fn dispatch(input: &str) -> String {
    String::from_utf8(chartgen_protocol::handle(input.as_bytes())).expect("JSON is UTF-8")
}
#[pymodule]
fn _native(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_function(wrap_pyfunction!(dispatch, module)?)?;
    module.add("PROTOCOL_VERSION", chartgen_protocol::VERSION)?;
    Ok(())
}
