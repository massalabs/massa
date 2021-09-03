# Ethereum JSON-RPC Specification Compatibility

API info at https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=false&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false

## Motivations

TODO

## Useful links:

**Metamask.io**

- all methods: https://docs.metamask.io/guide/rpc-api.html#ethereum-json-rpc-methods
- important methods: https://metamask.github.io/api-playground/api-documentation/

**Ethereum JSON RPC**

- wiki: https://eth.wiki/json-rpc/API
- playground: https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=true&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false

- explain why we refactor and why these decisions are been made -> https://gitlab.com/massalabs/massa/-/issues/149#note_667252470

## Ports used

3 types of interaction : admin, client and eth on different ports:

- metamask.io interface will be expose on port `31242` using https://docs.rs/jsonrpc-http-server
- massa client use 2 API running on ports `31243` (manager/admin) and `31244` (stats/info/user/wallet) using https://docs.rs/jsonrpc-core-client
- bootstrap run on port `31245`

# API V2 endpoints list (I/O action and eth_ mapping with our client)

* [ ] Step 1: Use wrapper to simplify API filters https://gitlab.com/massalabs/massa/-/issues/48
* [ ] Step 2: Do not use ApiEvent to interact with other controllers, use their command senders directly. This will improve performance and reduce programming overhead.
* [ ] Step 3: Use a shared structures crate for API <=> Client communication
* [ ] Step 4: Improve endpoints to factor better, reduce redundancy...

**Feature requests:**

- [ ] It would be nice to be able to see to which cliques a block belong for visualisation purposes. from @qdr
- [ ] Clique indices are not consistent from one call to the other. However, for now we can indicate whether a block is in the blockclique or not. from @damip
- [ ] If possible it would be nice to have the history of blocks created by an address. People are asking for this feature. from @qdr

## Block information object

defined as having the following properties:

- parentHash
- sha3Uncles
- miner
- stateRoot
- transactionsRoot
- receiptsRoot
- logsBloom
- difficulty
- number
- gasLimit
- gasUsed
- timestamp
- extraData
- mixHash
- nonce
- totalDifficulty
- baseFeePerGas
- size
- transactions
- uncles

## getBlockByHash.

## getBlockByNumber

## getBlockTransactionCountByHash

## getBlockTransactionCountByNumber

## getUncleCountByBlockHash

## getUncleCountByBlockNumber

## protocolVersion

## syncing

## coinbase

## accounts

## blockNumber

## Call

##  estimateGas

## gasPrice

## feeHistory

## newFilter

## newBlockFilter

## newPendingTransactionFilter

## uninstallFilter

## getFilterChanges

## getFilterLogs

## getLogs

## hashrate

## getWork

## submitWork

## submitHashrate

## sign

## signTransaction

## getBalance

## getStorage

## getTransactionCount

##  getCode

## sendTransaction

##  sendRawTransaction

##  getTransactionByHash

## getTransactionByBlockHashAndIndex

## getTransactionByBlockNumberAndIndex

## getTransactionReceipt

---

# API Endpoints

## Needs : 

- Debug
    - cliques:
        - input: (none)
        - output:
            - list of Clique:
                - blocks: [BlockId] list
                - fitness: u64
                - is_blockclique: bool
    - cliques stats:
        - input: (none)
        - output:
            - list of Clique stats:
                - block count : u64
                - fitness: u64
                - is_blockclique: bool
    - get block
        - input: Block Id
        - output: Block
    - get operation
        - input: Operation Id
        - output: Operation
    - get genesis diff
        - input none
        - relative time since/to genesis timestamp (in slots ?)
- Explorer 
    - get_config:
        - input: (None)
        - output ApiGetConfigDto:
            - t0
            - delta_f0
            - version
            - genesis_timestamp
            - roll_price
            - TODO architecture params
    - get_stats:
        - input: (None)
        - output:
            - server_timestamp: UTime 
            - last_slot: Slot (optional)
            - next_slot: Slot
            - time_stats:
                - time_start: UTime
                - time_end: UTime
                - final_block_count: u64
                - stale_block_count: u64
                - final_operation_count: u64
            - pool_stats:
                - operation_count: u64
                - endorsement_count: u64
            - network_stats:
                - in_connection_count: u64
                - out_connection_count: u64
                - known_peer_count: u64
                - banned_peer_count: u64
                - active_node_count: u64
    - get_network_info:
        - input: (None)
        - output:
            - node_ip
            - node_id
            - connected_nodes: [NodeInfo] where NodeInfo is:
                - id: NodeId
                - ip: IPAddress
                - is_outgoing: bool
                - last_success: UTime
                - last_failure: UTime
    - get_operations:
        - input: [OperationId] list
        - output: [OperationInfo] list where OperationInfo is:
            - id: OperationId
            - in_pool: bool
            - in_blocks: [BlockId] list
            - is_final: bool
            - operation: full Operation object
    - get_endorsements:
        - input: [EndorsementId] list
        - output: [EndorsementInfo] list where EndorsementInfo is:
            - id: EndorsementId
            - in_pool: bool
            - in_blocks: [BlockId] list
            - is_final: bool
            - endorsement: full Endorsement object
    - get_addresses:
        - input: [Address] list
        - output: [AddressInfo] list where AddressInfo is:
            - address: Address
            - thread: u8
            - balance:
                - final: Amount
                - candidate: Amount
            - blocks_created: [BlockId] list
            - involved_in_endorsements: [EndorsementId] list
            - involved_in_operations: [OperationId] list
    - get_block:
        - input [BlockId] list
        - output: [BlockInfo] list where BlockInfo is:
            - id: BlockId
            - is_final: bool
            - is_stale: bool
            - is_in_blockclique: bool
            - block: full Block object
    - get_graph_interval:
        - input:
            - (optional time_start: UTime)
            - (optional time_end: UTime)
        - output: [BlockSummary] list where BlockSummary is:
            - id: BlockId
            - is_final: bool
            - is_stale: bool
            - is_in_blockclique: bool
            - slot: Slot
            - creator: Address
            - parents: [BlockId] list

- User
    - get next draw (block and endorsement creation)
        - input [Address] list
        - output : [slot] list
    - get_operations:
        - input: [OperationId] list
        - output: [OperationInfo] list where OperationInfo is:
            - id: OperationId
            - in_pool: bool
            - in_blocks: [BlockId] list
            - is_final: bool
            - operation: full Operation object
    - get balance:
        - input [Address] list
        - output : for each address
            - candidate balance : 64
            - final balance : u64
            - locked balance : u64
            - candidate roll count : u64
            - final roll count : u64
- Admin
    - ban 
        - input : ipaddr or Node Id ?
        - output : none
    - unban
        - input : ipaddr or Node Id ?
        - output : none
    - start node
        - input : none
        - output : none
    - stop node
        - input : none
        - output : none
    - sign message 
        - input : [u8]
        - output : (signature, puclic key)
    - staking keys
        - add 
            - input Private keys
            - output None
        - remove
            - input : private keys
            - output : none
        - list
            - input none
            - output adresses
    - our_ip
        - input: none
        - output: ipaddr
    - peers
        - input none
        - output peer info list + node id

## Structure (old @damip proposal):

-   network_info

    -   Request: (empty)
    -   Response:
        -   node_id: NodeId
        -   routable_ip: Option
        -   connected_nodes: array\[\] of elements:
            -   node_id: NodeId
            -   ip: IpAddress
            -   is_outgoing: bool

-   consensus_info

    -   request: (empty)
    -   response:
        -   genesis_timestamp
        -   t0
        -   thread_count
        -   clique_count
        -   operation_validity_duration
        -   timestamp
        -   previous_slot: Option
        -   next_slot: Slot

-   block_graph

    -   request:
        -   start_timestamp: Option
        -   end_timestamp: Option
        -   filter_states: Array[] of elements:
            -   State (Active, Final, Discarded)
    -   response:
        -   items: Array[] of elements:
            -   ItemType (...)

# V1 API Specs

Here are the current **API endpoints**:

```rust
block -> Block
```
```rust
get_operations -> Vec<(OperationId, OperationSearchResult)>
```
```rust
blockinterval <start: Option> <end: Option> -> Vec<(BlockId, Slot)>
```
```rust
current_parents -> Vec<(BlockId, Slot)>
```
```rust
last_final -> Vec<(BlockId, Slot)>
```
```rust
graph_interval <start: Option> <end: Option> -> Vec<(BlockId, Slot, Status, Vec)>
```
```rust
cliques -> (usize, Vec<HashSet<(BlockId, Slot)>>
```
```rust
peers -> HashMap<IpAddr, PeerInfo>
```
```rust
our_ip -> Option
```
```rust
network_info -> Option, HashMap<IpAddr, PeerInfo>
```
```rust
node_config -> SerializationContext
```
```rust
pool_config -> PoolConfig
```
```rust
consensus_config -> ConsensusConfig
```
```rust
state -> State { 
    time: UTime, 
    latest_slot: Option, 
    current_cycle: u64, 
    our_ip: Option, 
    last_final: Vec<(BlockId, Slot, UTime)>, 
    nb_cliques: usize, 
    nb_peers: usize, 
}
```
```rust
last_stale -> Vec<(BlockId, Slot)>
```
```rust
last_invalid -> Vec<(BlockId, Slot)>
```
```rust
staker_info -> StakerInfo { 
    staker_active_blocks: Vec<(BlockId, BlockHeader)>, 
    staker_discarded_blocks: Vec<(BlockId, DiscardReason, BlockHeader)>,
    staker_next_draws: Vec, 
}
```
```rust
next_draws -> Vec<(Address, Slot)>
```
```rust
operations_involving_address -> HashMap<OperationId, OperationSearchResult>
```
```rust
addresses_info -> HashMap<Address, AddressState>
```
```rust
stop_node
```
```rust
register_staking_keys
```
```rust
remove_staking_addresses
```
```rust
send_operations Vec
```
```rust
get_stats -> ConsensusStats { 
    timespan: UTime, 
    final_block_count: u64, 
    final_operation_count: u64, 
    stale_block_count: u64, 
    clique_count: u64, 
}
```
```rust
active_stakers -> Option<HashMap<Address, u64>>
```
```rust
staking_addresses -> HashSet
```

Current **client CLI interface** (given by `--help`):

*  `quit`: quit Massa client
*  `help`: this help
*  `set_short_hash`: shorten displayed hashes: Parameters: bool: true (short), false(long)
*  `our_ip`: get node ip
*  `peers`: get node peers
*  `cliques`: get cliques
*  `current_parents`: get current parents
*  `last_final`: get latest finals blocks
*  `block`: get the block with the specified hash. Parameters: block hash
*  `blockinterval`: get blocks within the specified time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp
*  `graphinterval`: get the block graph within the specified time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp
*  `network_info`: network information: own IP address, connected peers
*  `version`: current node version
*  `state`: summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count
*  `last_stale`: (hash, thread, slot) for recent stale blocks
*  `staking_addresses`: hashset of  staking addresses
*  `last_invalid`: (hash, thread, slot, discard reason) for recent invalid blocks
*  `get_operation`: returns the operation with the specified id. Parameters: <operation id>
*  `stop_node`: Gracefully stop the node
*  `unban`: unban <ip address>
*  `staker_info`: staker info from staker address -> (blocks created, next slots in which the address will be selected)
*  `staker_stats`: production stats from staker address. Parameters: list of addresses separated by , (no space).
*  `register_staking_keys`: add a new private key for the node to use to stake
*  `remove_staking_addresses`: removes an address used to stake
*  `next_draws`: next draws for given addresses (list of addresses separated by ,  (no space))-> vec (address, slot for which address is selected)
*  `operations_involving_address`: list operations involving the provided address. Note that old operations are forgotten.
*  `block_ids_by_creator`: list blocks created by the provided address. Note that old blocks are forgotten.
*  `addresses_info`: returns the final and candidate balances for a list of addresses. Parameters: list of addresses separated by ,  (no space).
*  `cmd_testnet_rewards_program`: Returns rewards id. Parameter: <staking_address> <discord_ID> 
*  `get_active_stakers`: returns the active stakers and their roll counts for the current cycle.
*  `wallet_info`: Shows wallet info
*  `wallet_new_privkey`: Generates a new private key and adds it to the wallet. Returns the associated address.
*  `send_transaction`: sends a transaction from <from_address> to <to_address> (from_address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <from_address> <to_address> <amount> <fee>
*  `wallet_add_privkey`: Adds a list of private keys to the wallet. Returns the associated addresses. Parameters: list of private keys separated by ,  (no space).
*  `buy_rolls`: buy roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
*  `sell_rolls`: sell roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
