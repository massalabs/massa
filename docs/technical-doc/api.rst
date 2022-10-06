.. index:: api

.. _technical-api:

==================
Massa JSON-RPC API
==================

This crate exposes Rust methods (through the `Endpoints` trait) as
JSON-RPC API endpoints (thanks to the `ParityJSON-RPC <https://github.com/paritytech/jsonrpc>`_ crate).

**E.g.** this curl command will call endpoint `node_stop` (and stop the
locally running `massa-node`):

.. code-block:: bash

    curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "stop_node", "id": 123 }' 127.0.0.1:33034

You can interact with the Massa RPC API on `OpenRPC Playground <https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/massalabs/massa/main/docs/technical-doc/openrpc.json>`_.
