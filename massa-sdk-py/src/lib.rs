// use pyo3::exceptions;
use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use massa_models::address::Address;
use massa_models::amount::Amount;
use massa_models::datastore::Datastore;
use massa_models::slot::Slot;
use massa_models::datastore::DatastoreSerializer;
use massa_models::operation::{Operation, OperationDeserializer, OperationSerializer, OperationType};
use massa_models::config::constants;
use massa_serialization::{DeserializeError, Deserializer, Serializer};

// A custom struct (just for testing purpose)

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

// Slot (first example)

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

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("{}", self.inner))
    }

}

// Datastore

#[pyclass]
struct DatastoreIter {
    inner: std::collections::btree_map::IntoIter<Vec<u8>, Vec<u8>>
}

#[pymethods]
impl DatastoreIter {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<(Vec<u8>, Vec<u8>)> {
        slf.inner.next()
    }
}

#[pyclass]
#[derive(Clone)]
pub struct DatastorePy {
    pub inner: Datastore,
}

#[pymethods]
impl DatastorePy {

    #[new]
    pub fn new() -> Self {
        Self {
            inner: Default::default()
        }
    }

    pub fn insert(&mut self, key: Vec<u8>, value: Vec<u8>) {
        self.inner.insert(key, value);
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.inner))
    }

    fn __iter__(slf: PyRef<'_, Self>) -> PyResult<Py<DatastoreIter>> {
        let iter = DatastoreIter {
            inner: slf.inner.clone().into_iter()
        };
        Py::new(slf.py(), iter)
    }

    pub fn serialize(&self) -> PyResult<Vec<u8>> {
        let serializer = DatastoreSerializer::new();
        let mut buffer = Vec::new();
        serializer
            .serialize(&self.inner, &mut buffer)
            .map_err(|e|
                PyRuntimeError::new_err(e.to_string())
            )?;
        Ok(buffer)
    }
}

// Operation (enough to build an transaction op and serialize / deserialize)

#[pyclass]
#[derive(Clone)]
pub struct OperationTypePy {
    pub inner: OperationType,
}

#[pymethods]
impl OperationTypePy {

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.inner))
    }

    #[staticmethod]
    fn new_transaction(recipient_address: Vec<u8>, amount: u64) -> Self {
        // FIXME: here we use Vec<u8> as &[u8; 32] is not easily translated by PyO3
        //        But at the end, we would have AddressPy avail to deal with this
        Self {
            inner: OperationType::Transaction {
                recipient_address: Address::from_bytes(&recipient_address[..].try_into().unwrap()),
                amount: Amount::from_raw(amount),
            }
        }
    }
}

#[pyclass]
pub struct OperationPy {
    pub inner: Operation,
}

#[pymethods]
impl OperationPy {
    #[new]
    pub fn new(fee: u64, expire_period: u64, op_type: OperationTypePy) -> Self {
        Self {
            inner: Operation {
                fee: Amount::from_raw(fee),
                expire_period,
                op: op_type.inner
            }
        }
    }

    fn __repr__(&self) -> PyResult<String> {
        Ok(format!("{:?}", self.inner))
    }

    pub fn serialize(&self) -> PyResult<Vec<u8>> {
        let serializer = OperationSerializer::new();
        let mut buffer = Vec::new();
        serializer
            .serialize(&self.inner, &mut buffer)
            .map_err(|e|
                PyRuntimeError::new_err(e.to_string())
            )?;
        Ok(buffer)
    }

    #[staticmethod]
    pub fn deserialize(buffer: Vec<u8>) -> PyResult<OperationPy> {
        let deserializer = OperationDeserializer::new(
            constants::MAX_DATASTORE_VALUE_LENGTH,
            constants::MAX_FUNCTION_NAME_LENGTH,
            constants::MAX_PARAMETERS_SIZE,
            constants::MAX_OPERATION_DATASTORE_ENTRY_COUNT,
            constants::MAX_OPERATION_DATASTORE_KEY_LENGTH,
            constants::MAX_OPERATION_DATASTORE_VALUE_LENGTH);

        let (_bytes_left, operation) = deserializer
            .deserialize::<DeserializeError>(&buffer)
            .map_err(|e|
                PyRuntimeError::new_err(e.to_string())
            )?;
        Ok(OperationPy {
            inner: operation
        })
    }
}



/*
/// Formats the sum of two numbers as string.
#[pyfunction]
fn sum_as_string(a: usize, b: usize) -> PyResult<String> {
    Ok((a + b).to_string())
}
*/

/// A Python module implemented in Rust.
#[pymodule]
fn massa_sdk_py(_py: Python, m: &PyModule) -> PyResult<()> {
    // m.add_function(wrap_pyfunction!(sum_as_string, m)?)?;
    // m.add_class::<MySlot>()?;
    m.add_class::<SlotPy>()?;
    m.add_class::<DatastorePy>()?;
    m.add_class::<OperationPy>()?;
    m.add_class::<OperationTypePy>()?;
    Ok(())
}