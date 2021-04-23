use crate::contract_vm::engine::ContractInstance;
use crate::contract_vm::querier::WasmHandler;

pub mod analyzer;
pub mod editor;
pub mod engine;
pub mod mock;
pub mod querier;

pub fn build_simulation(
    wasmfile: &str,
    contract_addr: &str,
    sender_addr: &str,
    wasm_handler: WasmHandler,
    replicated_log: &[(String, String)],
) -> Result<ContractInstance, String> {
    let wasmer = engine::ContractInstance::new_instance(
        wasmfile,
        contract_addr,
        sender_addr,
        wasm_handler,
        replicated_log,
    );
    return wasmer;
}
