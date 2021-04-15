extern crate cosmwasm_std;
extern crate cosmwasm_vm;
extern crate serde_json;
use cosmwasm_std::{Coin, ContractResult, Uint128};
use cosmwasm_vm::{Instance, InstanceOptions, Size};

use crate::contract_vm::{analyzer, mock};
use cosmwasm_vm::testing::{mock_env, MockApi, MockQuerier, MockStorage};

use std::fmt::Write;
use wasmer_middleware_common::metering;
use wasmer_runtime_core::{
    backend::Compiler,
    codegen::{MiddlewareChain, StreamingCompiler},
    module::Module,
};
use wasmer_singlepass_backend::ModuleCodeGenerator as SinglePassMCG;

const DEFAULT_CONTRACT_BALANCE: u64 = 10_000_000_000_000_000;
const DEFAULT_GAS_LIMIT: u64 = 500_000_000_000_000;
const COMPILE_GAS_LIMIT: u64 = 10_000_000_000;
const DEFAULT_MEMORY_LIMIT: Size = Size::mebi(16);
const DEFAULT_PRINT_DEBUG: bool = true;

pub struct ContractInstance {
    pub module: Module,
    pub instance: Instance<MockApi, MockStorage, MockQuerier<mock::SpecialQuery>>,
    pub wasm_file: String,
    pub env: cosmwasm_std::Env,
    pub message: cosmwasm_std::MessageInfo,
    pub analyzer: analyzer::Analyzer,
}

fn compiler() -> Box<dyn Compiler> {
    let c: StreamingCompiler<SinglePassMCG, _, _, _, _> = StreamingCompiler::new(move || {
        let mut chain = MiddlewareChain::new();
        chain.push(metering::Metering::new(COMPILE_GAS_LIMIT));
        chain
    });
    Box::new(c)
}

impl ContractInstance {
    pub fn new_instance(wasm_file: &str, contract_addr: &str) -> Result<Self, String> {
        let balances = vec![Coin {
            denom: "orai".to_string(),
            amount: Uint128::from(DEFAULT_CONTRACT_BALANCE),
        }];
        let deps = mock::new_mock(balances.as_slice(), contract_addr);
        let wasm = match analyzer::load_data_from_file(wasm_file) {
            Err(e) => return Err(e),
            Ok(code) => code,
        };
        if cfg!(debug_assertions) {
            println!("Compiling code");
        }
        let md = wasmer_runtime_core::compile_with(wasm.as_slice(), compiler().as_ref()).unwrap();
        let inst_options = InstanceOptions {
            gas_limit: DEFAULT_GAS_LIMIT,
            /// Memory limit in bytes. Use a value that is divisible by the Wasm page size 65536, e.g. full MiBs.
            memory_limit: DEFAULT_MEMORY_LIMIT,
            print_debug: DEFAULT_PRINT_DEBUG,
        };
        let inst = match cosmwasm_vm::Instance::from_code(wasm.as_slice(), deps, inst_options) {
            Err(e) => {
                println!("cosmwasm_vm::Instance::from_code return error {}", e);
                return Err("Instance from code execute failed!".to_string());
            }
            Ok(i) => i,
        };
        return Ok(ContractInstance::make_instance(
            md,
            inst,
            wasm_file.to_string(),
            contract_addr,
            balances,
        ));
    }

    fn make_instance(
        md: Module,
        inst: cosmwasm_vm::Instance<MockApi, MockStorage, MockQuerier<mock::SpecialQuery>>,
        file: String,
        contract_addr: &str,
        contract_balance: Vec<Coin>,
    ) -> ContractInstance {
        return ContractInstance {
            module: md,
            instance: inst,
            wasm_file: file,
            env: ContractInstance::build_mock_env(),
            message: cosmwasm_std::MessageInfo {
                sender: cosmwasm_std::HumanAddr(contract_addr.to_string()),
                sent_funds: contract_balance,
            },
            analyzer: analyzer::Analyzer::default(),
        };
    }

    fn build_mock_env() -> cosmwasm_std::Env {
        let mut env = mock_env();
        env.block.chain_id = "Oraichain".to_string();
        env
    }

    pub fn show_module_info(&self) {
        println!("showing wasm module info for [{}]", self.wasm_file);
        println!("backend : [{}]", self.module.info().backend);

        println!("=============================== module info exported func name ===============================");
        for exdesc in self.module.exports() {
            println!("exported func name [{}]", exdesc.name);
        }
        println!("=============================== module info exported func name ===============================");
        for desc in self.module.imports() {
            println!("import descriptor name:[{}->{}]", desc.namespace, desc.name);
        }
    }

    fn dump_result(key: &str, value: &[u8]) {
        let mut value_str = match std::str::from_utf8(value) {
            Ok(result) => result.to_string(),
            _ => "".to_string(),
        };

        if value_str.is_empty() {
            for a in value.iter() {
                write!(value_str, "{:02x}", a).expect("Not written");
            }
        }

        println!("{} = {}", key, value_str);
    }
    pub fn call(&mut self, func_type: String, param: String) -> String {
        println!("***************************call started***************************");
        println!("executing func [{}] , params is {}", func_type, param);
        let gas_init = self.instance.get_gas_left();
        if func_type == "init" {
            let result = cosmwasm_vm::call_init::<_, _, _, cosmwasm_std::Empty>(
                &mut self.instance,
                &self.env,
                &self.message,
                param.as_bytes(),
            )
            .unwrap();

            match result {
                ContractResult::Ok(val) => {
                    for msg in &val.attributes {
                        ContractInstance::dump_result(&msg.key, msg.value.as_bytes());
                    }
                }
                ContractResult::Err(err) => println!("Contract Error: {}", err.to_string()),
            }
        } else if func_type == "handle" {
            let result = cosmwasm_vm::call_handle::<_, _, _, cosmwasm_std::Empty>(
                &mut self.instance,
                &self.env,
                &self.message,
                param.as_bytes(),
            )
            .unwrap();

            // println!("{:#?}", handle_result.unwrap());
            match result {
                ContractResult::Ok(val) => {
                    for msg in &val.attributes {
                        ContractInstance::dump_result(&msg.key, msg.value.as_bytes());
                    }
                }
                ContractResult::Err(err) => println!("Contract Error: {}", err.to_string()),
            }
        } else if func_type == "query" {
            // check param if it is custom, we will try to check for oracle special query to implement, otherwise forward
            // to virtual machine
            let result =
                cosmwasm_vm::call_query(&mut self.instance, &self.env, param.as_bytes()).unwrap();

            match result {
                ContractResult::Ok(val) => {
                    ContractInstance::dump_result("query data", val.as_slice());
                }
                ContractResult::Err(err) => println!("Contract Error: {}", err.to_string()),
            }
        } else {
            println!("wrong dispatcher call {}", func_type);
        }
        let gas_used = gas_init - self.instance.get_gas_left();
        println!("Gas used   : {}", gas_used);
        println!("***************************call finished***************************");
        return "Execute Success".to_string();
    }
}
