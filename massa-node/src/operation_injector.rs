use std::time::Duration;

use massa_models::{
    address::Address,
    amount::Amount,
    config::{T0, THREAD_COUNT},
    operation::{Operation, OperationType},
    timeslots::get_closest_slot_to_timestamp,
};
use massa_pool_exports::PoolController;
use massa_protocol_exports::ProtocolController;
use massa_signature::KeyPair;
use massa_storage::Storage;
use massa_time::MassaTime;
use massa_wallet::Wallet;

pub fn start_operation_injector(
    genesis_timestamp: MassaTime,
    storage: Storage,
    mut wallet: Wallet,
    mut pool_controller: Box<dyn PoolController>,
    protocol_controller: Box<dyn ProtocolController>,
    nb_op: u64,
) {
    let mut wait = genesis_timestamp
        .clone()
        .saturating_sub(MassaTime::now().unwrap())
        .saturating_add(MassaTime::from_millis(1000))
        .to_duration();
    if wait < Duration::from_secs(20) {
        wait = Duration::from_secs(20);
    }
    std::thread::sleep(wait);
    let return_addr = wallet
        .get_wallet_address_list()
        .iter()
        .next()
        .unwrap()
        .clone();
    let mut distant_wallets = vec![KeyPair::generate(0).unwrap(); 32];
    let mut wallets_created = vec![false; 32];
    let mut init_ops = vec![];
    while wallets_created.iter().any(|e| *e == false) {
        let keypair = KeyPair::generate(0).unwrap();
        let final_slot = get_closest_slot_to_timestamp(
            THREAD_COUNT,
            T0,
            genesis_timestamp,
            MassaTime::now().unwrap(),
        );
        let addr = Address::from_public_key(&keypair.get_public_key());
        let index: usize = addr.get_thread(THREAD_COUNT) as usize;
        if !wallets_created[index] {
            distant_wallets[index] = keypair;
            wallets_created[index] = true;
            init_ops.push(
                wallet
                    .create_operation(
                        Operation {
                            fee: Amount::const_init(0, 0),
                            expire_period: final_slot.period + 8,
                            op: OperationType::Transaction {
                                recipient_address: addr,
                                amount: Amount::const_init(10000, 0),
                            },
                        },
                        return_addr,
                    )
                    .unwrap(),
            )
        }
    }
    println!("Sending init ops len: {}", init_ops.len());
    let mut storage = storage.clone_without_refs();
    storage.store_operations(init_ops);
    pool_controller.add_operations(storage.clone());
    protocol_controller
        .propagate_operations(storage.clone())
        .unwrap();
    wallet.add_keypairs(distant_wallets.clone()).unwrap();
    std::thread::sleep(Duration::from_secs(10));
    std::thread::spawn(move || {
        use rand::Rng;
        let mut rng = rand::thread_rng();
        loop {
            let mut storage = storage.clone_without_refs();
            let txps = nb_op / 32;
            let final_slot = get_closest_slot_to_timestamp(
                THREAD_COUNT,
                T0,
                genesis_timestamp,
                MassaTime::now().unwrap(),
            );
            let mut ops = vec![];

            for i in 0..32 {
                for _ in 0..txps {
                    let amount = rng.gen_range(1..=10000);
                    let content = Operation {
                        fee: Amount::const_init(0, 0),
                        expire_period: final_slot.period + 8,
                        op: OperationType::Transaction {
                            recipient_address: return_addr,
                            amount: Amount::from_mantissa_scale(amount, 8).unwrap(),
                        },
                    };
                    let address = Address::from_public_key(&distant_wallets[i].get_public_key());
                    ops.push(wallet.create_operation(content, address).unwrap())
                }
            }
            storage.store_operations(ops);
            pool_controller.add_operations(storage.clone());
            protocol_controller
                .propagate_operations(storage.clone())
                .unwrap();
            std::thread::sleep(Duration::from_secs(1));
        }
    });
}
