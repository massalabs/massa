.. _testnet-install:

=================
Installing a node
=================

.. note::

    Right now 4 cores and 8 GB of RAM should be enough to run a node, but it might increase in the future. More info in the :ref:`FAQ <testnet-faq>`.

From binaries
=============

If you just wish to run a Massa node without compiling it yourself, you
can simply download the latest binary below and go the the next step: :ref:`Running a node <testnet-running>`.

- `Windows executable <https://github.com/massalabs/massa/releases/download/TEST.13.0/massa_TEST.13.0_release_windows.zip>`_
- `Linux binary <https://github.com/massalabs/massa/releases/download/TEST.13.0/massa_TEST.13.0_release_linux.tar.gz>`_ - only works with libc2.28 at least (for example Ubuntu 20.04 and higher)
- `MacOS binary <https://github.com/massalabs/massa/releases/download/TEST.13.0/massa_TEST.13.0_release_macos.tar.gz>`_

From source code
================

Otherwise, if you wish to run a Massa node from source code, here are the steps to follow:

On Ubuntu / MacOS
-----------------

- on Ubuntu, these libs must be installed: :code:`sudo apt install pkg-config curl git build-essential libssl-dev libclang-dev`
- on MacOS: :code:`brew install llvm`
- install `rustup <https://www.rust-lang.org/tools/install>`_: :code:`curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh`
- configure path: :code:`source $HOME/.cargo/env`
- check rust version: :code:`rustc --version`
- install `nigthly <https://doc.rust-lang.org/edition-guide/rust-2018/rustup-for-managing-rust-versions.html>`_: :code:`rustup toolchain install nightly`
- set it as default: :code:`rustup default nightly`
- check rust version: :code:`rustc --version`
- clone this repo: :code:`git clone --branch testnet https://github.com/massalabs/massa.git`

On Windows
----------

**Set up your Rust environment**

- On Windows, you should first follow the indications from Microsoft to be able to run on a Rust environment `here <https://docs.microsoft.com/en-gb/windows/dev-environment/rust/setup>`__.

  - Install Visual Studio (recommended) or the Microsoft C++ Build Tools.
  - Once Visual Studio is installed, click on C++ Build Tool. Select on the right column called "installation details" the following packages:

    - MSCV v142 -- VS 2019
    - Windows 10 SDK
    - C++ CMake tools for Windows
    - Testing Tools Core Feature

  - Click install on the bottom right to download and install those packages

- Install `Chocolatey <https://docs.chocolatey.org/en-us/choco/setup>`_ and run: :code:`choco install llvm`
- Install Rust, to be downloaded `here <https://www.rust-lang.org/tools/install>`__
- Install Git for windows, to be downloaded `here <https://git-scm.com/download/win>`__

**Clone the Massa Git Repository**

- Open Windows Power Shell

  - Clone the latest distributed version: :code:`git clone --branch testnet https://github.com/massalabs/massa.git`
  - Change default Rust to nightly: :code:`rustup default nightly`
