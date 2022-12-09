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
