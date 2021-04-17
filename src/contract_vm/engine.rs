extern crate cosmwasm_std;
extern crate cosmwasm_vm;
extern crate serde_json;

use colored::*;

use cosmwasm_std::{
    BlockInfo, Coin, ContractInfo, ContractResult, Empty, Env, HumanAddr, MessageInfo, Uint128,
};
use cosmwasm_vm::{Instance, InstanceOptions, Size};

use crate::contract_vm::querier::WasmHandler;
use crate::contract_vm::{analyzer, mock};
use cosmwasm_vm::testing::{MockApi, MockStorage};
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
const DENOM: &str = "orai";
const CHAIN_ID: &str = "Oraichain";

pub struct ContractInstance {
    pub module: Module,
    pub instance: Instance<MockApi, MockStorage, mock::MockQuerier<mock::SpecialQuery>>,
    pub wasm_file: String,
    pub env: Env,
    pub message: MessageInfo,
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
    pub fn new_instance(
        wasm_file: &str,
        contract_addr: &str,
        sender_addr: &str,
        query_wasm: WasmHandler,
    ) -> Result<Self, String> {
        let balances = vec![Coin {
            denom: DENOM.to_string(),
            amount: Uint128::from(DEFAULT_CONTRACT_BALANCE),
        }];

        let deps = mock::new_mock(balances.as_slice(), contract_addr, query_wasm);
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
                println!(
                    "cosmwasm_vm::Instance::from_code return error {}",
                    e.to_string().red()
                );
                return Err("Instance from code execute failed!".to_string());
            }
            Ok(i) => i,
        };
        return Ok(ContractInstance::make_instance(
            md,
            inst,
            wasm_file.to_string(),
            contract_addr,
            sender_addr,
            balances,
        ));
    }

    fn make_instance(
        md: Module,
        inst: cosmwasm_vm::Instance<MockApi, MockStorage, mock::MockQuerier<mock::SpecialQuery>>,
        file: String,
        contract_addr: &str,
        sender_addr: &str,
        sent_balances: Vec<Coin>,
    ) -> ContractInstance {
        return ContractInstance {
            module: md,
            instance: inst,
            wasm_file: file,
            env: Env {
                block: BlockInfo {
                    height: 12_345,
                    time: 1_571_797_419,
                    time_nanos: 879305533,
                    chain_id: CHAIN_ID.to_string(),
                },
                contract: ContractInfo {
                    address: HumanAddr::from(contract_addr),
                },
            },
            message: MessageInfo {
                sender: HumanAddr(sender_addr.to_string()),
                sent_funds: sent_balances,
            },
            analyzer: analyzer::Analyzer::default(),
        };
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

        println!("{} = {}", key.blue().bold(), value_str.yellow());
    }

    pub fn call(&mut self, func_type: String, param: String) -> String {
        println!("***************************call started***************************");
        println!(
            "executing func [{}] , params is {}",
            func_type.green().bold(),
            param.yellow()
        );
        let gas_init = self.instance.get_gas_left();
        if func_type == "init" {
            let result = cosmwasm_vm::call_init::<_, _, _, Empty>(
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
                ContractResult::Err(err) => println!("{}", err.to_string().red()),
            }
        } else if func_type == "handle" {
            let result = cosmwasm_vm::call_handle::<_, _, _, Empty>(
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
                ContractResult::Err(err) => println!("{}", err.to_string().red()),
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
                ContractResult::Err(err) => println!("{}", err.to_string().red()),
            }
        } else {
            println!("wrong dispatcher call {}", func_type.green().bold());
        }
        let gas_used = gas_init - self.instance.get_gas_left();
        println!("Gas used   : {}", gas_used.to_string().yellow());
        println!("***************************call finished***************************");
        return "Execute Success".to_string();
    }
}
