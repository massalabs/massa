/// Implementation of the interface used in the execution external library
///
use std::str::FromStr;

use crate::vm::CONTEXT;
use anyhow::bail;
use assembly_simulator::Interface;
use massa_models::Address;
use tokio::runtime::Runtime;

lazy_static::lazy_static! {
    pub(crate) static ref INTERFACE: Interface = Interface {
        get_module: |address| {
            let bytecode = Runtime::new().unwrap().block_on(async move {
                let context = CONTEXT.lock().await;
                context.ledger_step.get_module(&Address::from_str(address).unwrap()).await
            });
            match bytecode {
                Some(bytecode) => Ok(bytecode),
                _ => bail!("Error bytecode not found")
            }
        },
        ..Default::default()
    };
}
