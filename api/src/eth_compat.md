# Ethereum JSON-RPC Specification Compatibility

API info at https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=false&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false

## Motivations & feature requests:

* [x] Use wrapper to simplify API filters https://gitlab.com/massalabs/massa/-/issues/48
* [ ] Do not use ApiEvent to interact with other controllers, use their command senders directly. This will improve performance and reduce programming overhead.
* [x] Use a shared structures crate for API <=> Client communication: https://docs.rs/jsonrpc-derive/18.0.0/jsonrpc_derive/
* [ ] Improve endpoints to factor better, reduce redundancy...
* [ ] It would be nice to be able to see to which cliques a block belong for visualisation purposes. from @qdr
* [ ] Clique indices are not consistent from one call to the other. However, for now we can indicate whether a block is in the blockclique or not. from @damip
* [ ] If possible it would be nice to have the history of blocks created by an address. People are asking for this feature. from @qdr

## Useful links

**Metamask.io**

- all methods: https://docs.metamask.io/guide/rpc-api.html#ethereum-json-rpc-methods
- important methods: https://metamask.github.io/api-playground/api-documentation/

**Ethereum JSON RPC**

- wiki: https://eth.wiki/json-rpc/API
- playground: https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=true&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false

## Ports used

3 types of interaction : admin, client and eth on different ports:

- metamask.io interface will be expose on port `31242` using https://docs.rs/jsonrpc-http-server
- massa client (debug + explorer + user) use 2 API running on ports `31243` (manager/admin) and `31244` (stats/info/user/wallet) using https://docs.rs/jsonrpc-core-client
- bootstrap run on port `31245`

## API endpoints list (I/O action and `eth_` mapping with our client)

### Public

- MetaMask
  - `Call`
  - `accounts`
  - `getBalance`
  - `sendTransaction`
  - `sign`

- Debug (specific information)
    - `node_info`: We should fuse `our_ip`, `peers`, `get_network_info`, `get_config` into a single node_info command:
        - inputs (None)
        - output NodeInfo with fields:
            - node_id: NodeId
            - node_ip: Option<IpAddress>
            - version: Version
            - genesis_timestamp: UTime
            - t0
            - delta_f0
            - roll_price
            - thread_count
            - connected_nodes: HashMap<NodeId, IpAddress>
    - DEPRECIATED `our_ip`: get node ip `our_ip -> Option<IpAddr>`
        - input: none
        - output: ipaddr
    - DEPRECIATED `peers`: get node peers `peers -> HashMap<IpAddr, PeerInfo>`
        - input none
        - output peer info list + node id
    - DEPRECIATED `get_network_info`: network information: own IP address, connected peers `network_info() -> (Option<IpAddr>, NodeId, HashMap<IpAddr, NodeId, PeerInfo>)`
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
    - DEPRECIATED `get_config`: `get_config() -> ConfigDto;`
        - input: (None)
        - output ApiGetConfigDto:
            - t0
            - delta_f0
            - version
            - genesis_timestamp
            - roll_price
            - TODO architecture params
    - `get_cliques`: get cliques: `cliques -> Vec<Clique>`
        - input: (none)
        - output:
            - list of Clique:
                - blocks: [BlockId] list
                - fitness: u64
                - is_blockclique: bool
    - `get_block`: get the block with the specified hash. Parameters: block hash `get_block(BlockId) -> Block`
        - input: Block Id
        - output: Block
    - `get_operation`: returns the operation with the specified id. Parameters: <operation id> `get_operations(Vec<OperationId)>) -> Vec<(OperationId, OperationSearchResult)>`
        - input: Operation Id
        - output: Operation
    - `get_genesis`: `time_since_to_genesis() -> i64`
        - input none
        - relative time since/to genesis timestamp (in slots ?)
    - `get_graph_interval`: get the block graph within the specified time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp `graph_interval(start: Option<UTime>, end: Option<UTime>) -> Vec<BlockSummary>`
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
    - `get_endorsements`: `get endorsements(Vec<EndorsementId>) -> Vec<EndorsementInfo>`
        - input: [EndorsementId] list
        - output: [EndorsementInfo] list where EndorsementInfo is:
            - id: EndorsementId
            - in_pool: bool
            - in_blocks: [BlockId] list
            - is_final: bool
            - endorsement: full Endorsement object
    - TODO `get endorsement by id -> endorsement state { id: EndorsementId, in_pool: bool, in_blocks: [BlockId] list, is_final: bool, endorsement: full Endorsement object }`
    - TODO `operations_involving_address -> HashMap<OperationId, OperationSearchResult>`
    - TODO `addresses_info`: returns the final and candidate balances for a list of addresses. Parameters: list of addresses separated by, (no space). `addresses_info(Vec<Address>) -> HashMap<Address, AddressState>`
    - TODO `next_draws`: next draws for given addresses (list of addresses separated by, (no space)) -> vec (address, slot for which address is selected) `next_draws(Vec<Address>) -> Vec<(Address, Slot)>`
    - TODO `staker_info`: staker info from staker address -> (blocks created, next slots in which the address will be selected) `staker_info -> StakerInfo { staker_active_blocks: Vec<(BlockId, BlockHeader)>, staker_discarded_blocks: Vec<(BlockId, DiscardReason, BlockHeader)>, staker_next_draws: Vec }`
    - DEPRECIATED `blockinterval`: get blocks within the specified time interval. Optional parameters: [from] <start> (included) and [to] <end> (excluded) millisecond timestamp `blockinterval <start: Option> <end: Option> -> Vec<(BlockId, Slot)>`

- Explorer (aggregated stats)
    - `get_stats`: `get_stats -> ConsensusStats { timespan: UTime, final_block_count: u64, final_operation_count: u64, stale_block_count: u64, clique_count: u64 }`
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
    - `cliques_stats`: `clique_stats -> CliqueStats { block count: u64, fitness: u64, is_blockclique: bool }`
        - input: (none)
        - output:
            - list of Clique stats:
                - block count : u64
                - fitness: u64
                - is_blockclique: bool
    - `get_active_stakers`: returns the active stakers and their roll counts for the current cycle. `active_stakers -> Option<HashMap<Address, u64>>`
    - `get_operations`:
        - input: [OperationId] list
        - output: [OperationInfo] list where OperationInfo is:
            - id: OperationId
            - in_pool: bool
            - in_blocks: [BlockId] list
            - is_final: bool
            - operation: full Operation object
    - `get_addresses`: `addresses_info -> HashMap<Address, AddressState>`
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
    - `get_block`:
        - input [BlockId] list
        - output: [BlockInfo] list where BlockInfo is:
            - id: BlockId
            - is_final: bool
            - is_stale: bool
            - is_in_blockclique: bool
            - block: full Block object
    - DEPRECIATED `current_parents`: get current parents `current_parents -> Vec<(BlockId, Slot)>`
    - DEPRECIATED `last_final`: get latest finals blocks `last_final -> Vec<(BlockId, Slot)>`
    - DEPRECIATED `last_stale`: (hash, thread, slot) for recent stale blocks `last_stale -> Vec<(BlockId, Slot)>`
    - DEPRECIATED `last_invalid`: (hash, thread, slot, discard reason) for recent invalid blocks `last_invalid -> Vec<(BlockId, Slot)>`

- User (interaction with the node)
    - `get_next_draw` (block and endorsement creation)
        - input [Address] list
        - output : [slot] list
    - `get_operations`:
        - input: [OperationId] list
        - output: [OperationInfo] list where OperationInfo is:
            - id: OperationId
            - in_pool: bool
            - in_blocks: [BlockId] list
            - is_final: bool
            - operation: full Operation object
    - `get_balance`:
        - input [Address] list
        - output : for each address
            - candidate balance : 64
            - final balance : u64
            - locked balance : u64
            - candidate roll count : u64
            - final roll count : u64
    - `state`: summary of the current state: time, last final blocks (hash, thread, slot, timestamp), clique count, connected nodes count `state -> State { time: UTime, latest_slot: Option, current_cycle: u64, our_ip: Option, last_final: Vec<(BlockId, Slot, UTime)>, nb_cliques: usize, nb_peers: usize }`
    - TODO `send_operations Vec`

- TODO: Stats?
  * `version`: current node version
  * `operations_involving_address`: list operations involving the provided address. Note that old operations are forgotten. `operations_involving_address -> HashMap<OperationId, OperationSearchResult>`
  * `block_ids_by_creator`: list blocks created by the provided address. Note that old blocks are forgotten.
  * `staker_stats`: production stats from staker address. Parameters: list of addresses separated by , (no space).

### Private

- Admin/Manager?
    - `ban` (ip addr/node id) `ban(NodeId)`
        - input : ipaddr or Node Id ?
        - output : none
    - `unban`: unban <ip address> `unban(IpAddr)`
        - input : ipaddr or Node Id ?
        - output : none
    - `start_node`: `start_node()`
        - input : none
        - output : none
    - `stop_node`: Gracefully stop the node `stop_node()`
        - input : none
        - output : none
    - `sign_message`
        - input : [u8]
        - output : (signature, puclic key)
    - `staking_keys`: `staking_addresses -> HashSet`
        - `add`: add a new private key for the node to use to stake `register_staking_keys(Vec<PrivateKey>)`
            - input Private keys
            - output None
        - `remove`: removes an address used to stake `remove_staking_addresses(Vec<Address>)`
            - input : private keys
            - output : none
        - `list`: hashset of staking addresses `staking_addresses() -> HashSet<Address>`
            - input none
            - output adresses

- TODO: Wallet?
  * `wallet_info`: Shows wallet info
  * `wallet_new_privkey`: Generates a new private key and adds it to the wallet. Returns the associated address.
  * `send_transaction`: sends a transaction from <from_address> to <to_address> (from_address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <from_address> <to_address> <amount> <fee>
  * `wallet_add_privkey`: Adds a list of private keys to the wallet. Returns the associated addresses. Parameters: list of private keys separated by ,  (no space).
  * `buy_rolls`: buy roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
  * `sell_rolls`: sell roll count for <address> (address needs to be unlocked in the wallet). Returns the OperationId. Parameters: <address>  <roll count> <fee>
  * `cmd_testnet_rewards_program`: Returns rewards id. Parameter: <staking_address> <discord_ID>
  * TODO `node_config -> SerializationContext`
  * TODO `pool_config -> PoolConfig`
  * TODO `consensus_config -> ConsensusConfig`

## Needed Structures

```rust
pub struct OperationSearchResult {
    pub op: Operation,
    pub in_pool: bool,
    pub in_blocks: HashMap<BlockId, (usize, bool)>, // index, is_final
    pub status: OperationSearchResultStatus,
}

pub struct BlockSummary {
    pub id: BlockId,
    pub is_final: bool,
    pub is_stale: bool,
    pub is_in_blockclique: bool,
    pub slot: Slot,
    pub creator: Address,
    pub parents: Vec<BlockId>,
}

pub struct Clique {
    pub block_ids: HashSet<BlockId>,
    pub fitness: u64,
    pub is_blockclique: bool,
}

pub ConfigDto {
    pub t0: UTime
    pub delta_f0: u64
    pub version: Version
    pub genesis_timestamp: UTime
    pub roll_price: Amount
     TODO architecture params
 }

pub struct AddressState {
    pub final_rolls: u64,
    pub active_rolls: Option<u64>,
    pub candidate_rolls: u64,
    pub locked_balance: Amount,
    pub candidate_ledger_data: LedgerData,
    pub final_ledger_data: LedgerData,
}

pub struct EndorsementInfo{
    id: EndorsementId,
    in_pool: bool,
    in_blocks: Vec<BlockId>,
    is_final: bool,
    endorsement: Endorsement
}
```
