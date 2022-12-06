## Setup

python3 -m venv venv
venv/bin/python -m pip install maturin

## Building the sdk

source venv/bin/activate
maturin develop

## Examples 1: Operation

```python

from massa_sdk_py import OperationPy, OperationTypePy
op_tr = OperationTypePy.new_transaction([0]*32, 22)
print(repr(op_tr))
op1 = OperationPy(7, 33, op_tr)
print(repr(op1))
op1_ser = op1.serialize()
op1_2 = OperationPy.deserialize(op1_ser)
print(repr(op1_2))
```

## Examples 2: Datastore

```python
from massa_sdk_py import DatastorePy
ds = DatastorePy(); ds.insert([1, 2, 3], [255, 254, 253])
ds.insert([7, 8, 9], [255, 254, 253])
ds.insert([77, 88, 99], [25, 24, 23])
for (k, v) in ds:
  print(k, v)
print(repr(ds))
print(ds.serialize())
```
