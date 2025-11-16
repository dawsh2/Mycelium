use std::collections::HashMap;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use mycelium_ffi::{
    mycelium_publish, mycelium_runtime_create, mycelium_runtime_destroy, mycelium_subscribe,
    mycelium_unsubscribe, mycelium_verify_schema_digest, MyceliumRuntime, ERR_DIGEST,
    ERR_SUBSCRIPTION, SUCCESS,
};
use mycelium_protocol::SCHEMA_DIGEST;
use pyo3::exceptions::{PyRuntimeError, PyTypeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::{PyAny, PyBytes};
use pyo3::Bound;

#[pyclass(module = "mycelium_native", unsendable)]
struct Runtime {
    state: Arc<RuntimeState>,
}

#[pymethods]
impl Runtime {
    #[new]
    fn new(schema_digest: Option<&Bound<'_, PyAny>>) -> PyResult<Self> {
        let digest_buf = schema_digest
            .map(|obj| obj.extract::<Vec<u8>>())
            .transpose()?;
        let state = RuntimeState::new(digest_buf.as_deref())?;
        Ok(Self { state })
    }

    fn publish(&self, message: &Bound<'_, PyAny>) -> PyResult<()> {
        self.state.publish(message)
    }

    fn subscribe(
        &self,
        py: Python<'_>,
        message_cls: &Bound<'_, PyAny>,
        callback: PyObject,
    ) -> PyResult<Subscription> {
        let id = self.state.subscribe(py, message_cls, callback)?;
        Ok(Subscription::new(self.state.clone(), id))
    }

    fn close(&self) {
        self.state.close();
    }
}

impl Drop for Runtime {
    fn drop(&mut self) {
        self.state.close();
    }
}

#[pyclass(module = "mycelium_native", unsendable)]
struct Subscription {
    state: Arc<RuntimeState>,
    id: Option<u64>,
}

#[pymethods]
impl Subscription {
    fn close(&mut self) -> PyResult<()> {
        if let Some(id) = self.id.take() {
            self.state.unsubscribe(id)?;
        }
        Ok(())
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let _ = self.state.unsubscribe(id);
        }
    }
}

impl Subscription {
    fn new(state: Arc<RuntimeState>, id: u64) -> Self {
        Self {
            state,
            id: Some(id),
        }
    }
}

struct RuntimeState {
    handle: Mutex<Option<NonNull<MyceliumRuntime>>>,
    callbacks: Mutex<HashMap<u64, *mut CallbackState>>,
    closed: AtomicBool,
}

impl RuntimeState {
    fn new(user_digest: Option<&[u8]>) -> PyResult<Arc<Self>> {
        let handle_ptr = mycelium_runtime_create();
        let Some(ptr) = NonNull::new(handle_ptr) else {
            return Err(PyRuntimeError::new_err("failed to create mycelium runtime"));
        };

        let digest: &[u8] = user_digest.unwrap_or(&SCHEMA_DIGEST);
        let status =
            unsafe { mycelium_verify_schema_digest(ptr.as_ptr(), digest.as_ptr(), digest.len()) };
        if status != SUCCESS {
            unsafe { mycelium_runtime_destroy(ptr.as_ptr()) };
            if status == ERR_DIGEST {
                return Err(PyValueError::new_err("schema digest mismatch"));
            }
            return Err(PyRuntimeError::new_err(format!(
                "schema verification failed (status {status})"
            )));
        }

        Ok(Arc::new(Self {
            handle: Mutex::new(Some(ptr)),
            callbacks: Mutex::new(HashMap::new()),
            closed: AtomicBool::new(false),
        }))
    }

    fn publish(&self, message: &Bound<'_, PyAny>) -> PyResult<()> {
        let type_id: u16 = message
            .getattr("TYPE_ID")
            .map_err(|_| PyValueError::new_err("message missing TYPE_ID"))?
            .extract()?;

        let payload_obj = message
            .getattr("to_bytes")
            .map_err(|_| PyValueError::new_err("message missing to_bytes()"))?
            .call0()?;
        let payload: Bound<'_, PyBytes> = payload_obj.downcast_into()?;
        let payload_bytes = payload.as_bytes();

        let handle = self.handle_ptr()?;
        let status = unsafe {
            mycelium_publish(handle, type_id, payload_bytes.as_ptr(), payload_bytes.len())
        };

        map_status(status, "publish")
    }

    fn subscribe(
        &self,
        py: Python<'_>,
        message_cls: &Bound<'_, PyAny>,
        callback: PyObject,
    ) -> PyResult<u64> {
        let callback_ref = callback.bind(py);
        if !callback_ref.is_callable() {
            return Err(PyTypeError::new_err("callback must be callable"));
        }

        let type_id: u16 = message_cls
            .getattr("TYPE_ID")
            .map_err(|_| PyValueError::new_err("message class missing TYPE_ID"))?
            .extract()?;

        if message_cls.getattr("from_bytes").is_err() {
            return Err(PyValueError::new_err("message class missing from_bytes()"));
        }

        let state = Box::new(CallbackState {
            message_cls: message_cls.clone().unbind(),
            callback,
        });
        let user_data = Box::into_raw(state);

        let mut subscription_id = 0u64;
        let handle = self.handle_ptr()?;
        let status = unsafe {
            mycelium_subscribe(
                handle,
                type_id,
                Some(dispatch_callback),
                user_data.cast::<c_void>(),
                &mut subscription_id,
            )
        };

        if status != SUCCESS {
            unsafe {
                drop(Box::from_raw(user_data));
            }
            return Err(PyRuntimeError::new_err(format!(
                "subscribe failed (status {status})"
            )));
        }

        self.callbacks
            .lock()
            .expect("callbacks mutex poisoned")
            .insert(subscription_id, user_data);

        Ok(subscription_id)
    }

    fn unsubscribe(&self, subscription_id: u64) -> PyResult<()> {
        if self.closed.load(Ordering::SeqCst) {
            self.drop_callback(subscription_id);
            return Ok(());
        }

        let handle = match *self.handle.lock().expect("handle mutex poisoned") {
            Some(ptr) => ptr,
            None => {
                self.drop_callback(subscription_id);
                return Ok(());
            }
        };

        let status = unsafe { mycelium_unsubscribe(handle.as_ptr(), subscription_id) };
        self.drop_callback(subscription_id);

        if status == SUCCESS || status == ERR_SUBSCRIPTION {
            Ok(())
        } else {
            Err(PyRuntimeError::new_err(format!(
                "unsubscribe failed (status {status})"
            )))
        }
    }

    fn close(&self) {
        if self.closed.swap(true, Ordering::SeqCst) {
            return;
        }

        let handle = self.handle.lock().expect("handle mutex poisoned").take();
        {
            let mut callbacks = self.callbacks.lock().expect("callbacks mutex poisoned");
            for (_, ptr) in callbacks.drain() {
                unsafe {
                    drop(Box::from_raw(ptr));
                }
            }
        }

        if let Some(ptr) = handle {
            unsafe { mycelium_runtime_destroy(ptr.as_ptr()) };
        }
    }

    fn handle_ptr(&self) -> PyResult<*mut MyceliumRuntime> {
        let guard = self.handle.lock().expect("handle mutex poisoned");
        guard
            .map(|ptr| ptr.as_ptr())
            .ok_or_else(|| PyRuntimeError::new_err("runtime closed"))
    }

    fn drop_callback(&self, subscription_id: u64) {
        if let Some(ptr) = self
            .callbacks
            .lock()
            .expect("callbacks mutex poisoned")
            .remove(&subscription_id)
        {
            unsafe {
                drop(Box::from_raw(ptr));
            }
        }
    }
}

struct CallbackState {
    message_cls: Py<PyAny>,
    callback: Py<PyAny>,
}

unsafe extern "C" fn dispatch_callback(
    type_id: u16,
    payload_ptr: *const u8,
    payload_len: usize,
    user_data: *mut c_void,
) {
    if user_data.is_null() {
        return;
    }

    let state = &*(user_data as *const CallbackState);
    let payload = std::slice::from_raw_parts(payload_ptr, payload_len).to_vec();

    Python::with_gil(|py| {
        let message_result = (|| -> PyResult<_> {
            let message_cls = state.message_cls.bind(py);
            let bytes = PyBytes::new_bound(py, &payload);
            let message = message_cls.call_method1("from_bytes", (bytes,))?;
            Ok(message)
        })();

        match message_result {
            Ok(message) => {
                if let Err(err) = state.callback.call1(py, (message,)) {
                    err.print(py);
                    tracing::error!(type_id, "python callback raised");
                }
            }
            Err(err) => {
                err.print(py);
                tracing::error!(type_id, "failed to decode message for callback");
            }
        }
    });
}

fn map_status(code: i32, op: &str) -> PyResult<()> {
    if code == SUCCESS {
        Ok(())
    } else {
        Err(PyRuntimeError::new_err(format!(
            "{op} failed (status {code})"
        )))
    }
}

#[pymodule]
fn mycelium_native(py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    pyo3::prepare_freethreaded_python();
    m.add_class::<Runtime>()?;
    m.add_class::<Subscription>()?;
    m.add("SCHEMA_DIGEST", PyBytes::new_bound(py, &SCHEMA_DIGEST))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use pyo3::types::PyModule;

    #[test]
    fn callback_state_decodes_python_message() {
        pyo3::prepare_freethreaded_python();
        Python::with_gil(|py| {
            let module = PyModule::from_code_bound(
                py,
                r#"
captured = []

class Dummy:
    TYPE_ID = 42

    @staticmethod
    def from_bytes(payload):
        return payload.decode("ascii")

def capture(msg):
    captured.append(msg)
"#,
                "dummy.py",
                "dummy",
            )
            .expect("module");

            let state = Box::new(CallbackState {
                message_cls: module.getattr("Dummy").unwrap().unbind(),
                callback: module.getattr("capture").unwrap().unbind(),
            });
            let ptr = Box::into_raw(state);
            let payload = b"hi".to_vec();

            unsafe {
                dispatch_callback(42, payload.as_ptr(), payload.len(), ptr.cast());
            }

            let captured: Vec<String> = module.getattr("captured").unwrap().extract().unwrap();
            assert_eq!(captured, vec!["hi".to_string()]);

            unsafe {
                drop(Box::from_raw(ptr));
            }
        });
    }
}
