## Generating C bindings

    cd massa-models
    cargo build -j 8
    cbindgen --config cbindgen.toml -d --crate massa_models --output massa_models.h

## Building C program

```C
#include <stdio.h>
// #include "../../../massa2/massa-models/massa_models.h"
#include "massa_models.h"

int main() {
    // printf() displays the string inside quotation
    printf("Hello, World!\n");

    // Variables

    Slot* slot1 = NULL;
    Slot* slot1_der = NULL;
    // uint8_t* buffer = NULL;

    // End Variables

    // C playground

    uint8_t* buf0 = malloc(2);
    buf0[0] = 1;
    buf0[1] = 7;
    printf("buf0 values: %d %d\n", buf0[0], buf0[1]);
    free(buf0);

    // End C playground

    slot1 = new_slot(1, 7);
    printf("Successfully created new slot: %ld - %d\n", slot1->Period, slot1->Thread);

    struct CBuffer cbuffer = slot_serialize(slot1);
    printf("cbuffer len: %ld\n", cbuffer.Len);
    printf("cbuffer values: %d %d\n", cbuffer.Ptr[0], cbuffer.Ptr[1]);

    slot1_der = slot_deserialize(cbuffer);
    printf("Successfully der slot: %ld - %d\n", slot1_der->Period, slot1_der->Thread);

    free_slot(slot1);
    free_slot(slot1_der);

    return 0;
}
```

    LC_ALL="C" gcc main.c -o main_c_1 -I ../../../massa2/massa-models/ -L. -l:../../../massa2/target/debug/libmassa_models.so && ./main_c_1

### Valgrind check

    valgrind --leak-check=full --show-leak-kinds=all  ./main_c_1

## Python bindings (CFFI)

    python3 -m venv venv
    venv/bin/python -m pip install cffi

### Python bindings: tasks.py

```python
# tasks.py
import pathlib

import cffi

""" Build the CFFI Python bindings """
print("Building CFFI Module")
ffi = cffi.FFI()

this_dir = pathlib.Path().absolute()
h_file_name = this_dir / "massa_models.h"
with open(h_file_name) as h_file:
    ffi.cdef(h_file.read())

ffi.set_source(
    "massa_models_py",
    # Since you're calling a fully-built library directly, no custom source
    # is necessary. You need to include the .h files, though, because behind
    # the scenes cffi generates a .c file that contains a Python-friendly
    # wrapper around each of the functions.
    '#include "massa_models.h"',
    # The important thing is to include the pre-built lib in the list of
    # libraries you're linking against:
    libraries=["massa_models"],
    library_dirs=[this_dir.as_posix()],
    extra_link_args=["-Wl,-rpath,."],
)

ffi.compile()
```

Note: 
* Need (for now) manual processing of massa_models.h
  * Some "#define" are not supported by python-cffi
  * Can we automate this process?
    * COntribute to python-cffi project?

### Python bindings: howto use

```python
import massa_models_py

if __name__ == "__main__":

    print("Hello")
    print(dir(massa_models_py.lib))

    slot1 = massa_models_py.lib.new_slot(1, 7)
    slot1_ser = massa_models_py.lib.slot_serialize(slot1)
    print("slot1", slot1, slot1.Period, slot1.Thread);
    print("slot1 ser", slot1_ser)
    slot1_ser = massa_models_py.lib.slot_serialize(slot1)

    slot1_der = massa_models_py.lib.slot_deserialize(slot1_ser);
    print("slot1 der", slot1_der, slot1_der.Period, slot1_der.Thread)
```

