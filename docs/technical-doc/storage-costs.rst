========================
Storage Costs
========================

Explanations
------------

In Massaa the ledger is shared across all nodes of the network. We need to set a size limit to be able to run a node without
having 100TB of storage which will cause a barrier for adoption and running nodes at home.
We chose to limit the size to 1TB. Everyone can store data until the ledger reach 1TB. But how can we ensure this limit ?

We chose to force users to lock coins when they claim storage space and so we created a correlation with storage and circulating coins.

For each byte of storage you claim (for your address and balance, a key in your datastore, bytecode, ...) you need to lock coins. The coins are released when you release your space in the storage.

The amount of coins you need to lock for one byte is 0.00025 Massa. This value has been chosen so that if half of the coins (250 000 000 Massa) are locked, we will reach the 1TB.

As balances are stored as `varint <https://developers.google.com/protocol-buffers/docs/encoding#varints>`__ in the ledger their size can vary. To avoid difficulties and incomprehension we decided to use a fixed size for each balance. This size is 8 bytes and so initial ledger entry (address + balance) cost (8 + 32) * 0.00025 = 0.01 Massa .
Datastore keys also have a variable size and so we decided to use a fixed size of 10 bytes for calculating storage cost.

If you want to calculate the storage cost of your address in the ledger the formula is : address_size + balance_constant + bytecode_length + sum of (constants datastore key + value size) = 32 + 8 + bytecode.len() + sum_i(10 + datastore[i].len())) * 0.00025

The storage costs are always paid by the caller address. If you are using ABI like set_bycode_for you will be charged for the storage costs.

Example
-------

To create your address on the blockchain someone need to send at least 0.00025 * (32 + 8) = 0.01 Massa when sending the operation that will create your address (a transfer for example). This is the cost of creating your address and balance.

You want to store your birth date that is 30 bytes long in your datastore so you need to send an operation that will create a key in your datastore using a SC. This operation will cost you in storage costs at least 0.00025 * (10 + 30) = 0.01 Massa.

Now you want to delete this entry on your datastore then you will do it with a SC and you will be refunded of the storage costs (0.02 Massa).

