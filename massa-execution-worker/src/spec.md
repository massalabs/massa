We described here what should be done whan a bytecode call another
(spoted by get_module in the interface)

```rust

/// ABI allowing a contract to call another.
fn _call(shared_env: &SharedExecutionContext, addr: Address, func_name: String, max_gas: u64) {
    //TODO add arbitrary input parameters and return value

    //TODO metering / mem limit

    // prepare execution
    let old_max_gas;
    let old_coins;
    let target_module;
    let ledger_push;
    {
        let mut exec_context_guard = shared_env.0.lock().unwrap();

        // TODO make sure max_gas >= context.remaining_gas

        // get target module
        if let Some(module) = (*exec_context_guard).ledger_step._get_module(&addr) {
            target_module = module;
        } else {
            // no module to call
            // TODO error
            return;
        }

        // save old context values
        ledger_push = (*exec_context_guard).ledger_step.caused_changes.clone();
        old_max_gas = (*exec_context_guard).max_gas; // save old max gas
        old_coins = (*exec_context_guard).coins;

        // update context
        (*exec_context_guard).max_gas = max_gas;
        (*exec_context_guard).coins = AMOUNT_ZERO; // TODO maybe allow sending coins in the call
        (*exec_context_guard).call_stack.push_back(addr);
    }

    // run
    let mut run_failed = false;
    match Instance::new(&target_module, &ImportObject::new()) // TODO bring imports into the execution context (?)
        .map(|inst| inst.exports.get_function(&func_name).unwrap().clone())
        .map(|f| f.native::<(), ()>().unwrap()) // TODO figure out the "native" explicit parameters
        .map(|f| f.call())
    {
        Ok(_rets) => {
            // TODO check what to do with the return values.
        }
        Err(_err) => {
            // failed to find target func, or invalid parameters, or execution error
            run_failed = true;
        }
    }

    // unstack execution context
    {
        let mut exec_context_guard = shared_env.0.lock().unwrap();
        (*exec_context_guard).max_gas = old_max_gas;
        (*exec_context_guard).coins = old_coins;
        (*exec_context_guard).call_stack.pop_back();
        if run_failed {
            // if the run failed, cancel its consequences on the ledger
            (*exec_context_guard).ledger_step.caused_changes = ledger_push;
        }
    }
}

```