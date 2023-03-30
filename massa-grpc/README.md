### Massa gRPC API

In order to compile proto files, you must have the `protoc` compiler installed on your system. `protoc` is a protocol buffer compiler that can generate code in a variety of programming languages.

To check if you have `protoc` installed on your system, you can run the following command in your terminal:

```
protoc --version
```

If you see a version number printed out, then you have `protoc` installed. If not, you will need to download and install it.

Installing Protoc
-----------------

### macOS

To install `protoc` on macOS using Homebrew, run the following command:

```
brew install protobuf
```

### Linux

To install `protoc` on Linux, you can download the binary file for your architecture from the [official Protobuf releases page](https://github.com/protocolbuffers/protobuf/releases). Once downloaded, extract the contents of the archive and move the `protoc` binary to a location on your system PATH.

Alternatively, you can use your distribution's package manager to install `protoc`. On Ubuntu, for example, you can run:

```
sudo apt-get install protobuf-compiler
```

### Windows

To install `protoc` on Windows, you can download the binary file for your architecture from the [official Protobuf releases page](https://github.com/protocolbuffers/protobuf/releases). Once downloaded, extract the contents of the archive and move the `protoc` binary to a location on your system PATH.

After installing `protoc`, you should be able to compile proto files using the appropriate language-specific plugin (e.g. `protoc --go_out=./ path/to/my_proto_file.proto`).


After installing `protoc`, please verify that the `protoc` command is accessible by running `protoc --version` again.

Project build
-------------

The project is set up to automatically compile proto files during the build process using 
[massa-proto/build.rs](../massa-proto/build.rs).

When the project is built, `build.rs` is executed and it uses the `tonic-build` crate to generate Rust code from the proto files. The generated Rust code could be found in [massa-proto/src/](../massa-proto/src/).


VSCode integration
------------------

1- Install [vscode-proto3](https://marketplace.visualstudio.com/items?itemName=zxh404.vscode-proto3) extension.

2- The following settings contain a `protoc` configuration block:

```
{
    "rust-analyzer.procMacro.enable": true,
    "rust-analyzer.cargo.buildScripts.enable": true,
    "protoc": {
        "path": "/path/to/protoc",
        "compile_on_save": true,
        "options": [
            "{workspaceRoot}/massa-proto/proto/massa/*.proto",
            "--proto_path=${workspaceRoot}/massa-proto/proto/massa",
            "--proto_path=${workspaceRoot}/massa-proto/proto/third-party",
            "--descriptor_set_out=${workspaceRoot}/massa-proto/src/api.bin"
        ]
    }
}
```

3- Add the snippet above to `.vscode/settings.json`.
