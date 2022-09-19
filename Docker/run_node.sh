#!/bin/bash

# set -x
  
WORKDIR="/app"
CONFIG="$WORKDIR/massa-node/config"
BASECONFIG="$WORKDIR/massa-node/base_config"
PRIVKEY="$CONFIG/node_privkey.key"
WALLET="$CONFIG/staking_wallet.dat"
BRANCH=${NETWORK:="testnet"}

init_node() {             
    cd $WORKDIR/massa-node 
    echo -e "[network]\nroutable_ip = \"`wget -qO- eth0.me`\"" > $CONFIG/config.toml         
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

# set_variable() {

# }

if [[ $(ls $CONFIG | wc -l) -gt 0 && $(ls $BASECONFIG | wc -l) -gt 0 ]]
then 
    if [[ -f $PRIVKEY && -f $WALLET ]]
    then
        echo "Node launch"
        start_node
    else
        echo "Node initialization"
        init_node
    fi
else
    echo "Add configs"
    if [[ ! -d $CONFIG ]]
    then
        mkdir -p $CONFIG && cd $CONFIG
        curl -sO "https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/config/{info.md}"
    fi

    if [[ ! -d $BASECONFIG ]]
    then
        mkdir -p $BASECONFIG && cd $BASECONFIG
        curl -sO "https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/config.toml"
        curl -sO "https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/initial_ledger.json"
        curl -sO "https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/initial_peers.json"
        curl -sO "https://raw.githubusercontent.com/massalabs/massa/${BRANCH}/massa-node/base_config/initial_rolls.json"
    fi
    echo "Node initialization"
    init_node
fi
