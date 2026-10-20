use pyo3::prelude::*;

mod applications;
// mod routing;

#[pymodule]
fn fastapi_rs(m: &Bound<'_, PyModule>) -> PyResult<()> {
    let mut builder = tokio::runtime::Builder::new_multi_thread();
    builder.enable_all();
    let _ = pyo3_async_runtimes::tokio::init(builder);

    // Exposes `FastAPI` directly at `from fastapi_rs import FastAPI`
    m.add_class::<applications::FastAPI>()?;
    m.add_class::<applications::ASGIInstance>()?;
    Ok(())
}
