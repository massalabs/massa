# WASM VM

We want to use smart contracts encoded as WASM byte code.

## State of the art of WASM implementation

### WebAssembly 101

1) [Introduction - Rust and WebAssembly](https://rustwasm.github.io/docs/book/)
2) [Lin Clark, Author at Mozilla Hacks - the Web developer blog](https://hacks.mozilla.org/author/lclarkmozilla-com/)

The same folks at the Mozilla Hacks blog (including Alon Zakai who made emscripten which is a bit of a prequel to wasm) all talked to this conf last year: [WebAssembly Summit 2020](https://www.youtube.com/playlist?list=PL6ed-L7Ni0yQ1pCKkw1g3QeN2BQxXvCPK)

The thing that we will undoubtedly use is [Wasmer - The Universal WebAssembly Runtime](https://wasmer.io/) (because it is the most complete / adapted to our needs if I understand correctly what we are trying to do: "a decentralized AWS Lambda-like")

Official tools/docs could be found [here](https://github.com/WebAssembly)!

[Understanding WebAssembly text format](https://developer.mozilla.org/en-US/docs/WebAssembly/Understanding_the_text_format) from Mozilla

### ByteCode Alliance works

[ByteCode Alliance](https://bytecodealliance.org/) also maintains some nice tools:

Runtimes (that could be embedded):

- [WasmTime](https://wasmtime.dev/), a small and efficient runtime for WebAssembly & WASI
- [WebAssembly Micro Runtime](https://github.com/bytecodealliance/wasm-micro-runtime)
- [Lucet](https://github.com/bytecodealliance/lucet/), the Sandboxing WebAssembly Compiler

ABI Bindings (with our Massa crate)

- [Experimental WASI API bindings for Rust](https://crates.io/crates/wasi)

### Specs

#### WASM

- https://www.w3.org/TR/wasm-core/
- https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#type-section

#### WASI

https://wasi.dev/
> Note that everything here is a prototype, and while a lot of stuff works, there are numerous missing features and some rough edges. For example, networking support is incomplete.

#### WASM Interfaces Types

https://github.com/WebAssembly/interface-types/blob/main/proposals/interface-types/Explainer.md
> Note: This repository is now archived. The interface types proposal has changed quite a lot since this was first implemented and the trajectory of the proposal and the implementation within wasmtime no longer aligns with this repository.

https://hacks.mozilla.org/2019/08/webassembly-interface-types/

#### WITX

https://github.com/WebAssembly/WASI/blob/main/docs/witx.md

### Wasmer

https://wasmer.io/

- WASI: https://docs.rs/wasmer-wasi/ & https://github.com/wasmerio/wasmer/blob/master/examples/wasi.rs
- Interfaces Types: https://github.com/wasmerio/interface-types

```rust
use wasmer::{imports, Function, Instance, Module, Store, Value};

type Address = u64;
type Amount = u64;

fn main() -> anyhow::Result<()> {
    let module_wat = r#"
    ;; TODO: put some Python interpreter here!
    (module
    (type $t0 (func (param i32) (result i32)))
    (func $add_one (export "add_one") (type $t0) (param $p0 i32) (result i32)
        get_local $p0
        i32.const 1
        i32.add))
    "#;

    let store = Store::default();
    let module = Module::new(&store, &module_wat)?;

    // The module could import any massa specific API calls
    let import_object = imports! {
        "env" => {
            "massa_get_balance" => Function::new_native(&store, massa_get_balance)
        },
    };

    fn massa_get_balance(_: Address) -> Amount {
        todo!()
    }

    let instance = Instance::new(&module, &import_object)?;
    // this will execute the smart-contract "add-one"
    let add_one = instance.exports.get_function("add_one")?;
    // the context passed here is `&[Value::I32(42)]`
    let result = add_one.call(&[Value::I32(42)])?;
    assert_eq!(result[0], Value::I32(43));

    Ok(())
}
```

Tools

- https://wapm.io
- https://webassembly.sh/
- https://github.com/wasienv/wasienv

## How to pass/receive Rust types to/from wasm?

https://github.com/adrien-zinger/wasm-simulation/issues/2

### Solution A: use AssemblyScript little overhead

- https://github.com/adrien-zinger/wasmer-as
- https://github.com/near/assemblyscript-json

### Solution B: use WasiEnv

TODO

### Solution C: use WasmBindgen

TODO

## Massa ABI design proposal

```rust
struct Context(CallStack, BlockInfo);

trait API {

    fn random(seeds) -> u62;

    fn readDB(Address, key); -> impl Iter // key in sense of key-value store (like Redis)

    fn writeDB(Address, key, value);

    fn createData() -> Address;

    fn exec(Address, function_name: &str, args: [Value]) -> Value;

    fn time(Slot);
}
```

### Open questions

- how do we handle ABI versioning?! depreciation?! how to make it evolve?!

- can we break/invalidate an SC? in the test net? do we have to be always retro-compatible?

- how do we enable deterministic execution in Wasmer? metering?

### FAQ

**Q: Does anyone have the list of syscalls exposed by browsers to wasm VMs running in the browser on hand?**

V8 in chrome gives access to the same browser APIs whether it is JS code or WASM code! (but as the opposite, the syscalls offered by Node and Wasmtime are quite different) since we can call any JS function from loaded WASM code https://developer.mozilla.org/en-US/docs/WebAssembly/Using_the_JavaScript_API logically we have any browser API available in JS which can be called in WASM, i.e. https://developer.mozilla.org/en-US/docs/Web/API (for Firefox and obviously in Chrome it's noticeably different, welcome to the wonderful world of web programming)

This is the usage of call: https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#call on functions defined in the `import` function zone: https://github.com/sunfishcode/wasm-reference-manual/blob/master/WebAssembly.md#import-section

**Q: is the browser API specified somewhere?**

- web API (exhibition of web syscalls on the WASM side): https://www.w3.org/TR/wasm-web-api/
- JS interface (as seen on the JS side in the browser): https://www.w3.org/TR/wasm-js-api/

**Q: Have you any SC (Smart Contracts) examples of DApps?**

- https://ethereum.org/fr/developers/tutorials/erc20-annotated-code/
- https://www.cryptokitties.co/technical-details
- https://docs.uniswap.org/protocol/reference/smart-contracts

**Q: any article to go further?**

- https://bnjjj.medium.com/why-and-how-we-wrote-a-compiler-in-rust-blog-post-series-1-x-the-context-e2f83b10edb9
- https://radu-matei.com/blog/wasm-api-witx/
- https://frehberg.wordpress.com/webassembly-and-dynamic-memory/
- https://rustinblockchain.org/newsletters/rust-and-smart-contracts/
- https://medium.com/oasislabs/blockchain-flavored-wasi-50e3612b8eba
