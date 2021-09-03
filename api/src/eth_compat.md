# Ethereum JSON-RPC Specification Compatibility


API info at https://playground.open-rpc.org/?schemaUrl=https://raw.githubusercontent.com/ethereum/eth1.0-apis/assembled-spec/openrpc.json&uiSchema%5BappBar%5D%5Bui:splitView%5D=false&uiSchema%5BappBar%5D%5Bui:input%5D=false&uiSchema%5BappBar%5D%5Bui:examplesDropdown%5D=false


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

