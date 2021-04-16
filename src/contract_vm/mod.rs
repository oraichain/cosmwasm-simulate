use crate::contract_vm::engine::ContractInstance;

pub mod analyzer;
pub mod editor;
pub mod engine;
pub mod mock;
pub mod querier;

pub fn build_simulation(
    wasmfile: &str,
    contract_addr: &str,
    sender_addr: &str,
) -> Result<ContractInstance, String> {
    let wasmer = engine::ContractInstance::new_instance(wasmfile, contract_addr, sender_addr);
    return wasmer;
}
