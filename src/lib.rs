use pyo3::prelude::*;

mod applications;
// mod routing;

#[pymodule]
fn fastapi_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    // Exposes `FastAPI` directly at `from fastapi import FastAPI`
    m.add_class::<applications::FastAPI>()?;
    Ok(())
}