extern crate cosmwasm_std;
extern crate cosmwasm_vm;
extern crate serde_json;

use colored::*;

use cosmwasm_std::{
    Addr, Attribute, BlockInfo, Coin, ContractInfo, ContractResult, CosmosMsg, Empty, Env,
    MessageInfo, Timestamp, Uint128,
};

use cosmwasm_vm::{Instance, InstanceOptions, Size};

use crate::contract_vm::querier::WasmHandler;
use crate::contract_vm::{analyzer, mock};
use cosmwasm_vm::testing::MockApi;
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
pub const DENOM: &str = "orai";
pub const CHAIN_ID: &str = "Oraichain";
const SCHEMA_FOLDER: &str = "schema";

pub static mut BLOCK_HEIGHT: u64 = 12_345;
// callback handle for Handle Response, like send native balance, execute other smart contract
pub type CallBackHandler = fn(&str, Vec<CosmosMsg>) -> Vec<Attribute>;

pub struct ContractInstance {
    pub module: Module,
    pub instance: Instance<MockApi, mock::MockStorage, mock::MockQuerier<mock::SpecialQuery>>,
    pub wasm_file: String,
    pub env: Env,
    pub analyzer: analyzer::Analyzer,
    pub handle_callback: CallBackHandler,
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
        query_wasm: WasmHandler,
        storage: &mock::MockStorage,
        handle_callback: CallBackHandler,
    ) -> Result<Self, String> {
        let balances = &[Coin {
            denom: DENOM.to_string(),
            amount: Uint128::from(DEFAULT_CONTRACT_BALANCE),
        }];
        let deps = mock::new_mock(balances, contract_addr, query_wasm, storage.to_owned());

        let wasm = match analyzer::load_data_from_file(wasm_file) {
            Err(e) => return Err(e),
            Ok(code) => code,
        };
        if cfg!(debug_assertions) {
            println!("Compiling code [{}]", wasm_file.blue().bold());
        }

        // compile then init instance wasmer
        let md = match wasmer_runtime_core::compile_with(wasm.as_slice(), compiler().as_ref()) {
            Err(e) => {
                println!(
                    "wasmer_runtime_core::compile_with return error {}",
                    e.to_string().red()
                );
                return Err("Compile with code failed!".to_string());
            }
            Ok(m) => m,
        };

        let inst_options = InstanceOptions {
            gas_limit: DEFAULT_GAS_LIMIT,
            /// Memory limit in bytes. Use a value that is divisible by the Wasm page size 65536, e.g. full MiBs.
            print_debug: DEFAULT_PRINT_DEBUG,
        };
        let inst = match cosmwasm_vm::Instance::from_code(
            wasm.as_slice(),
            deps,
            inst_options,
            Some(DEFAULT_MEMORY_LIMIT),
        ) {
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
            handle_callback,
        ));
    }

    fn make_instance(
        md: Module,
        inst: cosmwasm_vm::Instance<
            MockApi,
            mock::MockStorage,
            mock::MockQuerier<mock::SpecialQuery>,
        >,
        file: String,
        contract_addr: &str,
        handle_callback: CallBackHandler,
    ) -> ContractInstance {
        let alz = analyzer::from_json_schema(&file, SCHEMA_FOLDER);

        unsafe {
            ContractInstance {
                module: md,
                instance: inst,
                wasm_file: file,
                env: Env {
                    block: BlockInfo {
                        height: BLOCK_HEIGHT,
                        time: Timestamp::from_nanos(1_571_797_419_879_305_533),
                        chain_id: CHAIN_ID.to_string(),
                    },
                    contract: ContractInfo {
                        address: Addr::unchecked(contract_addr),
                    },
                },
                analyzer: alz,
                handle_callback,
            }
        }
    }

    pub fn show_module_info(&self) {
        println!(
            "showing wasm module info for [{}]",
            self.wasm_file.blue().bold()
        );
        println!("backend : [{}]", self.module.info().backend.blue().bold());

        println!("=============================== module info exported func name ===============================");
        for exdesc in self.module.exports() {
            println!("exported func name [{}]", exdesc.name.blue().bold());
        }
        println!("=============================== module info exported func name ===============================");
        for desc in self.module.imports() {
            println!(
                "import descriptor name:[{}->{}]",
                desc.namespace.blue().bold(),
                desc.name.yellow().bold()
            );
        }
    }

    fn dump_results(attributes: &Vec<Attribute>) {
        let len = attributes
            .iter()
            .map(|k| k.key.len())
            .max()
            .unwrap_or_default();
        for msg in attributes {
            ContractInstance::dump_result(&msg.key, msg.value.as_bytes(), len);
        }
    }

    fn dump_result(key: &str, value: &[u8], len: usize) -> String {
        let mut value_str = match std::str::from_utf8(value) {
            Ok(result) => result.to_string(),
            _ => "".to_string(),
        };

        if value_str.is_empty() {
            for a in value.iter() {
                write!(value_str, "{:02x}", a).expect("Not written");
            }
        }

        println!(
            "{:<len$} = {}",
            key.blue().bold(),
            value_str.yellow(),
            len = len
        );

        value_str
    }

    pub fn instantiate(&mut self, param: &str, info: &MessageInfo) -> String {
        self.instantiate_raw(param.as_bytes(), info)
    }

    pub fn instantiate_raw(&mut self, param: &[u8], info: &MessageInfo) -> String {
        let result = cosmwasm_vm::call_instantiate::<_, _, _, Empty>(
            &mut self.instance,
            &self.env,
            info,
            param,
        );

        match result {
            Ok(response) => match response {
                ContractResult::Ok(val) => {
                    ContractInstance::dump_results(&(self.handle_callback)(
                        self.env.contract.address.as_str(),
                        val.messages,
                    ));

                    ContractInstance::dump_results(&val.attributes);

                    // simulate block height increase for later expire check
                    unsafe {
                        BLOCK_HEIGHT += 1;
                        self.env.block.height = BLOCK_HEIGHT;
                    }

                    r#"{"message":"init succeeded"}"#.to_string()
                }
                ContractResult::Err(err) => {
                    println!("{}", err.red());
                    format!(r#"{{"error":"{}"}}"#, err)
                }
            },
            Err(err) => {
                println!("{}", err.to_string().red());
                format!(r#"{{"error":"{}"}}"#, err.to_string())
            }
        }
    }

    pub fn execute(&mut self, param: &str, info: &MessageInfo) -> String {
        self.execute_raw(param.as_bytes(), info)
    }

    pub fn execute_raw(&mut self, param: &[u8], info: &MessageInfo) -> String {
        let result =
            cosmwasm_vm::call_execute::<_, _, _, Empty>(&mut self.instance, &self.env, info, param);

        match result {
            Ok(response) => match response {
                ContractResult::Ok(val) => {
                    ContractInstance::dump_results(&(self.handle_callback)(
                        self.env.contract.address.as_str(),
                        val.messages,
                    ));

                    ContractInstance::dump_results(&val.attributes);

                    // simulate block height increase for later expire check
                    unsafe {
                        BLOCK_HEIGHT += 1;
                        self.env.block.height = BLOCK_HEIGHT;
                    }

                    r#"{"message":"handle succeeded"}"#.to_string()
                }
                ContractResult::Err(err) => {
                    println!("{}", err.red());
                    format!(r#"{{"error":"{}"}}"#, err)
                }
            },

            Err(err) => {
                println!("{}", err.to_string().red());
                format!(r#"{{"error":"{}"}}"#, err.to_string())
            }
        }
    }

    pub fn query(&mut self, param: &str) -> String {
        self.query_raw(param.as_bytes())
    }

    pub fn query_raw(&mut self, param: &[u8]) -> String {
        // check param if it is custom, we will try to check for oracle special query to implement, otherwise forward
        // to virtual machine
        let result = cosmwasm_vm::call_query(&mut self.instance, &self.env, param);

        match result {
            Ok(response) => match response {
                ContractResult::Ok(val) => {
                    ContractInstance::dump_result("query data", val.as_slice(), 10)
                }
                ContractResult::Err(err) => {
                    println!("{}", err.red());
                    format!(r#"{{"error":"{}"}}"#, err)
                }
            },
            Err(err) => {
                println!("{}", err.to_string().red());
                format!(r#"{{"error":"{}"}}"#, err.to_string())
            }
        }
    }

    pub fn call(&mut self, func_type: &str, param: &str, info: &MessageInfo) -> String {
        println!();
        println!("===========================call started===========================");
        println!(
            "executing func [{}] , params is {}",
            func_type.green().bold(),
            param.yellow()
        );
        let gas_init = self.instance.get_gas_left();
        let res = match func_type {
            "instantiate" => self.instantiate(param, info),
            "execute" => self.execute(param, info),
            "query" => self.query(param),
            _ => {
                println!("wrong dispatcher call {}", func_type.green().bold());
                format!(r#"{{"error":"wrong dispatcher call {}"}}"#, func_type)
            }
        };

        let gas_used = gas_init - self.instance.get_gas_left();
        println!(
            "{}   : {}",
            "gas used".blue().bold(),
            gas_used.to_string().yellow()
        );
        println!("===========================call finished===========================");
        println!();
        return res;
    }
}
