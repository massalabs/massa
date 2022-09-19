#!/bin/bash

WORKDIR="/app"
MN_BASEDIR="$WORKDIR/massa-node"
MC_BASEDIR="$WORKDIR/massa-client"
PRIVKEY="$MN_BASEDIR/config/node_privkey.key"
WALLET="$MN_BASEDIR/config/staking_wallet.dat"
BRANCH=${NETWORK:="testnet"}

init_node() {  
    echo "Add configs for massa-node"
    mkdir -p {$MN_BASEDIR/config,$MN_BASEDIR/base_config,$MN_BASEDIR/storage}    
    wget -P $MN_BASEDIR/config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/config/info.md
    wget -P $MN_BASEDIR/base_config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/config.toml
    wget -P $MN_BASEDIR/base_config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/initial_ledger.json
    wget -P $MN_BASEDIR/base_config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/initial_peers.json
    wget -P $MN_BASEDIR/base_config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/initial_rolls.json

    echo "Add configs for massa-client"
    mkdir -p {$MC_BASEDIR/config,$MC_BASEDIR/base_config}    
    wget -P $MC_BASEDIR/config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-client/config/info.md
    wget -P $MC_BASEDIR/base_config https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-client/base_config/config.toml

    echo -e "[network]\nroutable_ip = \"`wget -qO- eth0.me`\"" > $MN_BASEDIR/config/config.toml  

    cd $MN_BASEDIR            
    expect -c "
        #!/usr/bin/expect -f
        set timeout -1

        spawn ./massa-node
        exp_internal 0
        expect \"Enter new password for staking keys file:\"
        send   \"$PASSWORD\n\"
        expect \"Confirm password:\"
        send   \"$PASSWORD\n\"
        expect eof
    "
}

start_node() { 
    cd $WORKDIR/massa-node
    ./massa-node -p $PASSWORD
}

if [[ $LOGGING ]]
then
    set -x
fi

if [[ -f $PRIVKEY && -f $WALLET ]]
then
    echo "Node launch"
    start_node
else
    echo "Node initialization"
    init_node
fi
