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

const DEFAULT_CONTRACT_BALANCE: u64 = 10_000_000_000_000_000;
const DEFAULT_GAS_LIMIT: u64 = 500_000_000_000_000;
const DEFAULT_MEMORY_LIMIT: Size = Size::mebi(16);
pub const DENOM: &str = "orai";
pub const CHAIN_ID: &str = "Oraichain";
const SCHEMA_FOLDER: &str = "schema";

// Instance
const DEFAULT_INSTANCE_OPTIONS: InstanceOptions = InstanceOptions {
    gas_limit: DEFAULT_GAS_LIMIT,
    print_debug: false,
};

pub static mut BLOCK_HEIGHT: u64 = 12_345;
// callback execute for Handle Response, like send native balance, execute other smart contract
pub type CallBackHandler = fn(&str, Vec<CosmosMsg>) -> Vec<Attribute>;

pub struct ContractInstance {
    pub instance: Instance<MockApi, mock::MockStorage, mock::MockQuerier<mock::SpecialQuery>>,
    pub wasm_file: String,
    pub env: Env,
    pub analyzer: analyzer::Analyzer,
    pub execute_callback: CallBackHandler,
}

impl ContractInstance {
    pub fn new_instance(
        wasm_file: &str,
        contract_addr: &str,
        query_wasm: WasmHandler,
        storage: &mock::MockStorage,
        execute_callback: CallBackHandler,
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

        let inst = match cosmwasm_vm::Instance::from_code(
            wasm.as_slice(),
            deps,
            DEFAULT_INSTANCE_OPTIONS,
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
            inst,
            wasm_file,
            contract_addr,
            execute_callback,
        ));
    }

    fn make_instance(
        inst: cosmwasm_vm::Instance<
            MockApi,
            mock::MockStorage,
            mock::MockQuerier<mock::SpecialQuery>,
        >,
        file: &str,
        contract_addr: &str,
        execute_callback: CallBackHandler,
    ) -> ContractInstance {
        let alz = analyzer::from_json_schema(file, SCHEMA_FOLDER);

        unsafe {
            ContractInstance {
                instance: inst,
                wasm_file: file.to_string(),
                env: Env {
                    block: BlockInfo {
                        height: BLOCK_HEIGHT,
                        time: Timestamp::from_seconds(1_571_797_419),
                        chain_id: CHAIN_ID.to_string(),
                    },
                    contract: ContractInfo {
                        address: Addr::unchecked(contract_addr),
                    },
                    transaction: None,
                },
                analyzer: alz,
                execute_callback,
            }
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
                    ContractInstance::dump_results(&(self.execute_callback)(
                        self.env.contract.address.as_str(),
                        val.messages.into_iter().map(|msg| msg.msg).collect(),
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
                    ContractInstance::dump_results(&(self.execute_callback)(
                        self.env.contract.address.as_str(),
                        val.messages.into_iter().map(|msg| msg.msg).collect(),
                    ));

                    ContractInstance::dump_results(&val.attributes);

                    // simulate block height increase for later expire check
                    unsafe {
                        BLOCK_HEIGHT += 1;
                        self.env.block.height = BLOCK_HEIGHT;
                    }

                    r#"{"message":"execute succeeded"}"#.to_string()
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
