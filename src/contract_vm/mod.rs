use crate::contract_vm::engine::ContractInstance;

pub mod analyzer;
pub mod engine;
pub mod mock;
pub mod watcher;

pub fn build_simulation(wasmfile: &str) -> Result<ContractInstance, String> {
    let wasmer = engine::ContractInstance::new_instance(wasmfile);
    return wasmer;
}
