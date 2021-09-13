// Copyright (c) 2021 MASSA LABS <info@massa.net>

// Compiling jsonrpc-client-transports v18.0.0
// error[E0599]: no function or associated item named `new` found for struct `Client` in the current scope
//   --> /home/yvan/.cargo/registry/src/github.com-1ecc6299db9ec823/jsonrpc-client-transports-18.0.0/src/transports/http.rs:32:23
//    |
// 32 |     let client = Client::new();
//    |                          ^^^ function or associated item not found in `Client<_, _>`

fn main() {}
