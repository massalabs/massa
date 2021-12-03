# Transactions

Once you have coins in your wallet, here is how to transfer them.

First, check the balance of your addresses with this command in the
client:

```plain
wallet_info
```

You can send a transaction from an address in your wallet that has a
positive balance to any address with this command:

```plain
send_transaction <from_address> <to_address> <amount> <fee>
```

Change with the origin and destination addresses, and set the amount and
fee, for instance:

```plain
send_transaction 5y1JC9eEgCkXJXWcrCxzegZaUHZ1CcKEpAYMdAHPzyDLqUvvE 2NZ25sfyN7R4UWGUFU1H6N7UqYJh8NRMMoRBev4fU27BkxhBHy 23.1 0
```

As long as the blocks are not full, you can put a 0 fee.

Now check your balance. Your candidate balance should be lower, and
after \~40 sec the transaction will be final.

```plain
wallet_info
```
