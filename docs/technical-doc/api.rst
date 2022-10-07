.. index:: api

.. _technical-api:

==================
Massa JSON-RPC API
==================

This crate exposes Rust methods (through the `Endpoints` trait) as
JSON-RPC API methods (thanks to the `ParityJSON-RPC <https://github.com/paritytech/jsonrpc>`_ crate).

Massa JSON-RPC API is splitted in two parts : 

**Private API**: used for node management. Default port: 33034 e.g. http://localhost:33034

**Public API**: used for blockchain interactions. Default port: 33035 e.g. http://localhost:33035

Find the complete Massa `OpenRPC <https://spec.open-rpc.org/>`_  specification `here <https://raw.githubusercontent.com/massalabs/massa/main/docs/technical-doc/openrpc.json>`_.


Example
=======

The curl command below will create a JSON-RPC `request <https://www.jsonrpc.org/specification#request_object>`_ which calls `node_stop` method and stops the
locally running `massa-node`:

.. code-block:: bash

    curl -X POST -H "Content-Type: application/json" -d '{"jsonrpc": "2.0", "method": "stop_node", "id": 123 }' http://localhost:33034

Result: 

.. code-block:: json

    {
    "jsonrpc": "2.0",
    "result": 123,
    "id": 1
    }

Integrations
============

**JavaScript**: use `massa-web3.js <https://github.com/massalabs/massa-web3>`_.

**Smart contracts**: use `massa-sc-library <https://github.com/massalabs/massa-sc-library>`_.

**Playground**: use `Massa Playground <https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/massalabs/massa/main/docs/technical-doc/openrpc.json&uiSchema[appBar][ui:input]=false&uiSchema[appBar][ui:inputPlaceholder]=Enter Massa JSON-RPC server URL&uiSchema[appBar][ui:logoUrl]=https://massa.net/favicons/favicon.ico&uiSchema[appBar][ui:splitView]=false&uiSchema[appBar][ui:darkMode]=false&uiSchema[appBar][ui:title]=Massa&uiSchema[appBar][ui:examplesDropdown]=false&uiSchema[methods][ui:defaultExpanded]=false&uiSchema[methods][ui:methodPlugins]=true&uiSchema[params][ui:defaultExpanded]=false>`_.
