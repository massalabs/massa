use pyo3::prelude::*;
use pyo3::types::PyString;
use massa_models::slot::Slot;

#[pyclass]
struct MySlot {
    #[pyo3(get, set)]
    pub a: i32,
    #[pyo3(get, set)]
    pub b: i32,
}

#[pymethods]
impl MySlot {
    #[new]
    #[args(num = "-1")]
    fn new(a1: i32) -> Self {
        Self {
            a: a1,
            b: Default::default()
        }
    }
}

#[pyclass]
#[derive(Clone)]
pub struct SlotPy {
    pub inner: Slot,
}

#[pymethods]
impl SlotPy {
    #[new]
    pub fn new(period: u64, thread: u8) -> Self {
        Self {
            inner: Slot {
                period,
                thread,
            },
        }
    }

    fn __str__(&self) -> &'static str {
        "SlotPy"
    }

    /*
    fn __iter__(slf: PyRef<self>) -> PyResult<&PyAny> {
        PyString::new(slf.py(), "hello, world").iter()
    }
    */
}


/*
#[pyproto]
impl<'a> PyObjectProtocol for SlotPy {
    fn __str__(&self) -> PyResult<&'static str> {
        Ok("SlotPy")
    }

    fn __repr__(&'a self) -> PyResult<String> {
        Ok(format!("Slot py: {} {}", self.inner.period, self.inner.thread))
    }

    fn __getattr__(&'a self, name: &str) -> PyResult<String> {
        let out: String = match name {
            "period" => self.inner.period.into(),
            "thread" => self.inner.thread.into(),
            &_ => "INVALID FIELD".to_owned(),
        };
        Ok(out)
    }
}
*/

/// Formats the sum of two numbers as string.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}

/// A Python module implemented in Rust.
#[pymodule]
fn massa_sdk_py(_py: Python, m: &PyModule) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    m.add_class::<MySlot>()?;
    m.add_class::<SlotPy>()?;
    Ok(())
}