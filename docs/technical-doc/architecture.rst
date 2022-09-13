Architecture
============

We describe here the global architecture of a Massa Node, from the ground up. The goal of the Massa network is to gather, order and execute **operations**. These operations are produced by clients and sent to the Massa network via a **node**. Nodes will gather the pending operations and group them inside **blocks**, which are continuously constructed as new operations become available. 

Unlike traditional blockchains, Massa blocks are not simply chained one after the other, but organized into a more complexe spatio-temporal structure. Instead of one chain, there are several threads (T=32) running in parallel, with blocks equally spread on each threads over time and stored inside **slots** that are spaced at fixed time intervals:

.. image:: structure.drawio.svg

The time between two slots on the same thread is called a **period**, and lasts 16s (conventionnally called t0). Threads are slightly shifted in time relative to one another, by one period divided by the number of threads, which is 16s/32 = 0.5s, so that a period contains exactly 32 slots. A **cycle** is defined as the succession of 128 periods. Periods are numbered, so can be used together with a thread number to uniquely identify a block slot.

The job of the Massa nodes is to essentially fill up slots with valid blocks. To do so, at each interval of 0.5s, a specific node in the network is elected to be allowed to create a block (more about the selection process below), and will be rewarded for this if it creates a valid block. It is also possible that a node misses its opportunity to create the block, in which case the slot will remain empty (this is called a **block miss**).

In traditional blockchains, blocks are simply referencing their parents. In the case of Massa, each block is referencing one parent block in each thread (so, 32 parents). Here is an example illustrated with one particular block:

.. image:: block_parents.drawio.svg

The work done to build these blocks, endorse them, and run the whole network is performed by a node. This is the diagram of the architecture, where the pool and factories are running in potentially different processes. Otherwise, each of the modules described here runs inside one or more threads inside the same executable (NB: the pool+factories separation is not yet implemented, but will be soon)

.. image:: architecture.drawio.svg

Let's explain the different concepts and modules present in this diagram, and relevant definitions.

Address
*******

Each account in Massa has a public and private key associated to it. This is how messages can be signed
and identity enforced. 

The address of an account is, for now, simply the hash of the public key.

Operation
*********

The point of the Massa network is to gather, order and execute operations, stored inside blocks. There are basically three types of operations: transactions, roll operations and smart contract code execution. The general structure of an operation is the following, and the different types of operations differ by their payload:

===============================  =========================================================
**Operation header**       
------------------------------------------------------------------------------------------ 
``pubkey_creator`` (64 bytes)    The public key of the operation creator              
``expiration period P``          Period P after which the operation is expired        
``max_gas``                      The maximum gas spendable for this operation         
``fee``                          The amount of fees the creator is willing to pay     
``payload``                      The content of the operation (see below)            
``signature``                    signature of all the above with the private key of    
                                 the operation creator                                
===============================  =========================================================

Transactions operations
^^^^^^^^^^^^^^^^^^^^^^^

Transactions are operations that move native massa coins between addresses. Here is the corresponding payload:

===============================  =========================================================
**Transaction payload**       
------------------------------------------------------------------------------------------ 
``amount``                       The amount of coins to transfer              
``destination address``          The address of the recipient                        
===============================  =========================================================

Buy/Sell Rolls operations
^^^^^^^^^^^^^^^^^^^^^^^^^

Rolls are staking tokens that participants can buy or sell via native coins (more about staking below). This is done via special operations, whith a simple payload:

===============================  =========================================================
**Roll buy/sell payload**       
------------------------------------------------------------------------------------------ 
``nb_of_rolls``                  The number of rolls to buy or to sell              
===============================  =========================================================


Smart Contract operations
^^^^^^^^^^^^^^^^^^^^^^^^^

Smart Contracts are pieces of code that can be run in the Massa virtual machine (more about this below). There are two ways of calling for the execution of code:

1. Direct execution of bytecode

In the case, the code is provided in the operation payload and executed directly:

===============================  =========================================================
**Execute SC payload**       
------------------------------------------------------------------------------------------ 
``bytecode``                     The bytecode to run (in the context of the caller address)              
===============================  =========================================================

2. Smart Contract function call

Here, the code is indirectly called via the call to an existing smart contract function, together with the required parameters:

===============================  =========================================================
**Call SC**       
------------------------------------------------------------------------------------------ 
``target_address``               The address of the targetted smart contract
``target_fun``                   The function that is called              
``params``                       The parameters of the function call              
===============================  =========================================================

