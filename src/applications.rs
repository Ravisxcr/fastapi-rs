use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use pyo3::prelude::*;
use pyo3::types::PyCFunction;

#[pyclass]
#[derive(Clone)]
pub struct FastAPI {
    // Wrapped in Arc<Mutex<...>> so the closure can safely store handlers
    routes: Arc<Mutex<HashMap<String, PyObject>>>,
}

#[pymethods]
impl FastAPI {
    #[new]
    fn new() -> Self {
        Self {
            routes: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Implements `@app.get("/path")`
    fn get<'py>(&self, py: Python<'py>, path: String) -> PyResult<Bound<'py, PyCFunction>> {
        let routes = Arc::clone(&self.routes);

        // PyO3 0.22 Bound closure constructor
        PyCFunction::new_closure_bound(
            py,
            None,
            None,
            move |args, _kwargs| -> PyResult<PyObject> {
                // args[0] is the Python function passed to @app.get("/...")
                let handler = args.get_item(0)?;
                
                // Store the callback in the route map
                if let Ok(mut map) = routes.lock() {
                    map.insert(path.clone(), handler.clone().unbind());
                }

                // Return the handler back to Python so the decorated function remains callable
                Ok(handler.unbind())
            },
        )
    }

    /// Dispatch route for testing
    fn dispatch(&self, py: Python<'_>, path: &str) -> PyResult<PyObject> {
        let routes = self.routes.lock().map_err(|e| {
            pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
        })?;

        if let Some(handler) = routes.get(path) {
            let res = handler.call0(py)?;
            Ok(res)
        } else {
            Err(pyo3::exceptions::PyKeyError::new_err(format!("Route not found: {}", path)))
        }
    }
}