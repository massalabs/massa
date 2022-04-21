.. index:: library, types

.. _sc-types:

Types
=====

The following `AssemblyScript <https://www.assemblyscript.org>`_ types can be helpful in your smart contract journey without having to reinvent the wheel.

.. note::
   You know a nugget that could be added to this list or you have a specific need for a new type?
   `Open an issue <https://github.com/massalabs/massa-sc-library/issues>`_ to discuss about it!

.. _Currency:

Currency
--------

A representation of a monetary unit used to express a value.

Usage
^^^^^

.. code-block:: typescript

    import {Currency} from 'mscl-type';

    const euro = new Currency("Euro", 2);
    const yen = new Currency("Japanese yen", 0);
    const isSame = euro.sameAs(yen); // False

More info at `module repository <https://github.com/massalabs/massa-sc-library/tree/main/type>`_.

.. _Amount:

Amount
------

A representation of a value in a :ref:`Currency`.

.. warning::

    `Amount` implements :ref:`Valider` as some operations, such as subtraction leading to a negative value, can result in an invalid `Amount`.

Usage
^^^^^

.. code-block:: typescript

    import {Currency} from 'mscl-type';
    import {Amount} from 'mscl-type';

    const euro = new Currency("Euro", 2);

    const price = new Amount(500, euro);
    const accountBalance = new Amount(100, euro);

    cont isEnough = price.lessThan(accountBalance); // False
    const isValidAmount = accountBalance.substract(price).isValid(); // False

More info at `module repository <https://github.com/massalabs/massa-sc-library/tree/main/type>`_.

.. _Valider:

Valider
-------

An interface to unify how invalid types are handled.

.. note::

   * `Exception handling proposal <https://github.com/WebAssembly/exception-handling/blob/main/proposals/exception-handling/Exceptions.md>`_ is not yet implemented in `Wasmer <https://webassembly.org/roadmap>`_ or in `AssemblyScript <https://www.assemblyscript.org/status.html>`_;
   * `Result` type is not implemented;
   Then this is the only way to perform an action on a type and check later if the type is still valid.

Usage
^^^^^

.. code-block:: typescript

    import {Valider} from 'mscl-type';

    export MyAwesomeType implements Valider {
        ...
        isValid():bool {
            // check if the type is still valid
        }
    }
    ...

More info at `module repository <https://github.com/massalabs/massa-sc-library/tree/main/type>`_.
