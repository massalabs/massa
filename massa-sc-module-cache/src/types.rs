use massa_sc_runtime::RuntimeModule;

pub type ModuleInfo = (RuntimeModule, Option<u64>);

/// NOTE: this will replace ModuleInfo
pub enum ModuleInfoBis {
    Invalid,
    Module(RuntimeModule),
    ModuleAndDelta((RuntimeModule, u64)),
}
