use std::str::FromStr;

use massa_models::address::Address;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MasOGBalanceResponse {
    pub balances: Vec<MasOGBalance>,
    pub total_supply: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct MasOGBalance {
    pub final_balance: u128,
    pub candidate_balance: u128,
    pub voting_power: f64,
    pub address: Address,
}

pub(crate) struct VotingConfig {
    pub governance_sc_addr: Address,
    pub mas_og_addr: Address,
}

impl VotingConfig {
    pub fn new(chainid: u64) -> Self {
        match chainid {
            // Buildnet
            77658366 => VotingConfig {
                governance_sc_addr: Address::from_str(
                    "AS12PXh66grB66GQiLasV1nJcGPzTJhCYFaCGuwZQuEHBSePuxjpb",
                )
                .unwrap(),
                mas_og_addr: Address::from_str(
                    "AS1XFQaiw9wrHxizvrLKywKXxtyM9eHmmD2mKzvHXUqB2FCTNQpL",
                )
                .unwrap(),
            },
            // Mainnet
            77658377 => VotingConfig {
                governance_sc_addr: Address::from_str(
                    "AS12gXJzJra1kHUXftuBLyPrcfEBTJHz6L2nV5aNMnLmLnyBeJxDC",
                )
                .unwrap(),
                mas_og_addr: Address::from_str(
                    "AS12dwsZrv5dGUcVvKrLuFfrTy39izPf56cZSoBhkYFwd3BUM9tYu",
                )
                .unwrap(),
            },
            77 => VotingConfig {
                governance_sc_addr: Address::from_str(
                    "AS1zpwF1bMrieP7KFfEi4mpkip5FHGcCgtta9TfYEJHos2mFDw8n",
                )
                .unwrap(),
                mas_og_addr: Address::from_str(
                    "AS1F1jckcz7pG42vSYYjYGAZiB6WzAdkSEvhAwXvygeoGzGLA79G",
                )
                .unwrap(),
            },
            _ => panic!("Unsupported chainid: {}", chainid),
        }
    }
}
