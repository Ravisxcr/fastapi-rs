use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use pyo3::prelude::*;
use pyo3::types::{PyBytes, PyCFunction, PyDict, PyList, PyTuple};

async fn send_msg(send: &PyObject, dict: PyObject) -> PyResult<()> {
    let future = Python::with_gil(|py| {
        let coro = send.bind(py).call1((dict,))?;
        pyo3_async_runtimes::tokio::into_future(coro)
    })?;
    future.await?;
    Ok(())
}

async fn receive_msg(receive: &PyObject) -> PyResult<PyObject> {
    let future = Python::with_gil(|py| {
        let coro = receive.bind(py).call0()?;
        pyo3_async_runtimes::tokio::into_future(coro)
    })?;
    let res = future.await?;
    Ok(res)
}

fn handle_asgi<'py>(
    py: Python<'py>,
    routes: Arc<Mutex<HashMap<String, PyObject>>>,
    scope: &Bound<'py, PyDict>,
    receive: PyObject,
    send: PyObject,
) -> PyResult<Bound<'py, PyAny>> {
    let scope_type: String = match scope.get_item("type")? {
        Some(val) => val.extract()?,
        None => return Err(pyo3::exceptions::PyKeyError::new_err("Missing 'type' in ASGI scope")),
    };

    if scope_type == "lifespan" {
        return pyo3_async_runtimes::tokio::future_into_py(py, async move {
            loop {
                let msg = receive_msg(&receive).await?;
                let msg_type: String = Python::with_gil(|py| -> PyResult<String> {
                    msg.bind(py).get_item("type")?.extract()
                })?;

                if msg_type == "lifespan.startup" {
                    let resp = Python::with_gil(|py| -> PyResult<PyObject> {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("type", "lifespan.startup.complete")?;
                        Ok(dict.into())
                    })?;
                    send_msg(&send, resp).await?;
                } else if msg_type == "lifespan.shutdown" {
                    let resp = Python::with_gil(|py| -> PyResult<PyObject> {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("type", "lifespan.shutdown.complete")?;
                        Ok(dict.into())
                    })?;
                    send_msg(&send, resp).await?;
                    break;
                } else {
                    break;
                }
            }
            Ok(())
        });
    }

    if scope_type == "http" {
        let path: String = match scope.get_item("path")? {
            Some(val) => val.extract()?,
            None => "/".to_string(),
        };

        return pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Drain request body
            loop {
                let msg = receive_msg(&receive).await?;
                let more_body: bool = Python::with_gil(|py| {
                    msg.bind(py)
                        .get_item("more_body")
                        .and_then(|v| v.extract::<bool>())
                        .unwrap_or(false)
                });
                if !more_body {
                    break;
                }
            }

            // Check routes
            let handler_opt = Python::with_gil(|py| -> PyResult<Option<PyObject>> {
                let map = routes.lock().map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("Mutex poisoned: {}", e))
                })?;
                Ok(map.get(&path).map(|h| h.clone_ref(py)))
            })?;

            match handler_opt {
                Some(handler) => {
                    let res_obj = Python::with_gil(|py| -> PyResult<PyObject> {
                        Ok(handler.bind(py).call0()?.unbind())
                    })?;

                    let is_coro: bool = Python::with_gil(|py| {
                        let inspect = py.import_bound("inspect")?;
                        inspect.call_method1("iscoroutine", (res_obj.bind(py),))?.extract::<bool>()
                    }).unwrap_or(false);

                    let final_res = if is_coro {
                        let future = Python::with_gil(|py| {
                            pyo3_async_runtimes::tokio::into_future(res_obj.into_bound(py))
                        })?;
                        future.await?
                    } else {
                        res_obj
                    };

                    let body_bytes: Vec<u8> = Python::with_gil(|py| -> PyResult<Vec<u8>> {
                        let bound = final_res.bind(py);
                        if let Ok(s) = bound.extract::<String>() {
                            Ok(s.into_bytes())
                        } else if let Ok(b) = bound.extract::<Vec<u8>>() {
                            Ok(b)
                        } else {
                            let json = py.import_bound("json")?;
                            let s: String = json.call_method1("dumps", (bound,))?.extract()?;
                            Ok(s.into_bytes())
                        }
                    })?;

                    let start_dict = Python::with_gil(|py| -> PyResult<PyObject> {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("type", "http.response.start")?;
                        dict.set_item("status", 200)?;
                        let headers = PyList::empty_bound(py);
                        headers.append((
                            PyBytes::new_bound(py, b"content-type"),
                            PyBytes::new_bound(py, b"application/json"),
                        ))?;
                        dict.set_item("headers", headers)?;
                        Ok(dict.into())
                    })?;
                    send_msg(&send, start_dict).await?;

                    let body_dict = Python::with_gil(|py| -> PyResult<PyObject> {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("type", "http.response.body")?;
                        dict.set_item("body", PyBytes::new_bound(py, &body_bytes))?;
                        dict.set_item("more_body", false)?;
                        Ok(dict.into())
                    })?;
                    send_msg(&send, body_dict).await?;
                }
                None => {
                    let not_found = b"{\"detail\":\"Not Found\"}";
                    let start_dict = Python::with_gil(|py| -> PyResult<PyObject> {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("type", "http.response.start")?;
                        dict.set_item("status", 404)?;
                        let headers = PyList::empty_bound(py);
                        headers.append((
                            PyBytes::new_bound(py, b"content-type"),
                            PyBytes::new_bound(py, b"application/json"),
                        ))?;
                        dict.set_item("headers", headers)?;
                        Ok(dict.into())
                    })?;
                    send_msg(&send, start_dict).await?;

                    let body_dict = Python::with_gil(|py| -> PyResult<PyObject> {
                        let dict = PyDict::new_bound(py);
                        dict.set_item("type", "http.response.body")?;
                        dict.set_item("body", PyBytes::new_bound(py, not_found))?;
                        dict.set_item("more_body", false)?;
                        Ok(dict.into())
                    })?;
                    send_msg(&send, body_dict).await?;
                }
            }

            Ok(())
        });
    }

    // Default: unsupported protocol
    pyo3_async_runtimes::tokio::future_into_py(py, async move {
        Ok(())
    })
}

#[pyclass]
pub struct ASGIInstance {
    routes: Arc<Mutex<HashMap<String, PyObject>>>,
    scope: Py<PyDict>,
}

#[pymethods]
impl ASGIInstance {
    fn __call__<'py>(
        &self,
        py: Python<'py>,
        receive: PyObject,
        send: PyObject,
    ) -> PyResult<Bound<'py, PyAny>> {
        let scope = self.scope.bind(py);
        handle_asgi(py, Arc::clone(&self.routes), scope, receive, send)
    }
}

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

    /// Supports both ASGI 2 (`app(scope) -> instance(receive, send)`)
    /// and ASGI 3 (`app(scope, receive, send) -> coroutine`)
    #[pyo3(signature = (*args))]
    fn __call__<'py>(
        &self,
        py: Python<'py>,
        args: &Bound<'py, PyTuple>,
    ) -> PyResult<Bound<'py, PyAny>> {
        match args.len() {
            1 => {
                let scope: Bound<'py, PyDict> = args.get_item(0)?.extract()?;
                let instance = ASGIInstance {
                    routes: Arc::clone(&self.routes),
                    scope: scope.unbind(),
                };
                Bound::new(py, instance).map(|b| b.into_any())
            }
            3 => {
                let scope: Bound<'py, PyDict> = args.get_item(0)?.extract()?;
                let receive: PyObject = args.get_item(1)?.extract()?;
                let send: PyObject = args.get_item(2)?.extract()?;
                handle_asgi(py, Arc::clone(&self.routes), &scope, receive, send)
            }
            n => Err(pyo3::exceptions::PyTypeError::new_err(format!(
                "FastAPI expected 1 (ASGI 2) or 3 (ASGI 3) arguments, got {}",
                n
            ))),
        }
    }
}
