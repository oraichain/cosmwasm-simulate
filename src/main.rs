#![feature(proc_macro_hygiene, decl_macro)]

pub mod contract_vm;

extern crate clap;

use crate::contract_vm::analyzer::{Member, INDENT};
use crate::contract_vm::editor::TerminalEditor;
use crate::contract_vm::engine::{ContractInstance, BLOCK_HEIGHT, CHAIN_ID, DENOM};
use crate::contract_vm::mock::MockStorage;
use crate::contract_vm::querier::WasmHandler;

use clap::{App, Arg};
use colored::*;
use cosmwasm_std::{
    from_slice, Addr, Attribute, Binary, Coin, CosmosMsg, MessageInfo, QuerierResult, SystemError,
    SystemResult, Uint128, WasmMsg, WasmQuery,
};
use itertools::sorted;
use rocket::fairing::{Fairing, Info, Kind};
use rocket::http::Header;
use rocket::response::content;
use rocket::{Request, Response};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::path::Path;
use std::{fs, sync, thread, time, vec};

// default const is 'static lifetime
const DEFAULT_SENDER_ADDR: &str = "fake_sender_addr";
const DEFAULT_REPLICATED_LIMIT: usize = 1024;
const DEFAULT_SENDER_BALANCE: u64 = 10_000_000_000_000_000;

lazy_mut! {
    static mut EDITOR: TerminalEditor = TerminalEditor::new();
    static mut ENGINES : HashMap<String, ContractInstance> = HashMap::new();
    static mut ACCOUNTS : Vec<MessageInfo> = Vec::new();
}

#[macro_use]
extern crate rocket;

fn call_engine(
    contract_addr: &str,
    func_type: &str,
    msg: &str,
    account: Option<String>,
) -> Result<String, String> {
    unsafe {
        // addr must not be empty
        let mut addr = account.unwrap_or_default();
        if addr.is_empty() {
            addr = DEFAULT_SENDER_ADDR.to_string();
        };

        match ACCOUNTS.iter().find(|x| x.sender.to_string().eq(&addr)) {
            Some(info) => match ENGINES.get_mut(contract_addr) {
                None => Err(format!("No such contract: {}", contract_addr)),
                Some(engine) => match Binary::from_base64(msg) {
                    Ok(input) => match String::from_utf8(input.0) {
                        Ok(param) => Ok(engine.call(func_type, param.as_str(), info).to_owned()),
                        Err(err) => Err(err.to_string()),
                    },
                    Err(err) => Err(err.to_string()),
                },
            },
            None => Err(format!("No account found for {}", addr)),
        }
    }
}

#[get("/contract/<address>/instantiate/<msg>?<account>")]
fn instantiate_contract(
    address: String,
    msg: String,
    account: Option<String>,
) -> content::Json<String> {
    match call_engine(address.as_str(), "instantiate", msg.as_str(), account) {
        Ok(data) => content::Json(format!(r#"{{"data": {}}}"#, data)),
        Err(err) => content::Json(format!(r#"{{"error": "{}"}}"#, err)),
    }
}

#[get("/contract/<address>/execute/<msg>?<account>")]
fn execute_contract(
    address: String,
    msg: String,
    account: Option<String>,
) -> content::Json<String> {
    match call_engine(address.as_str(), "execute", msg.as_str(), account) {
        Ok(data) => content::Json(format!(r#"{{"data": {}}}"#, data)),
        Err(err) => content::Json(format!(r#"{{"error": "{}"}}"#, err)),
    }
}

#[get("/contract/<address>/query/<msg>")]
fn query_contract(address: String, msg: String) -> content::Json<String> {
    match call_engine(address.as_str(), "query", msg.as_str(), None) {
        Ok(data) => content::Json(format!(r#"{{"data": {}}}"#, data)),
        Err(err) => content::Json(format!(r#"{{"error": "{}"}}"#, err)),
    }
}

// empty struct
pub struct CORS;

impl Fairing for CORS {
    fn info(&self) -> Info {
        Info {
            name: "Add CORS headers to requests",
            kind: Kind::Response,
        }
    }

    fn on_response(&self, _request: &Request, response: &mut Response) {
        response.set_header(Header::new("Access-Control-Allow-Origin", "*"));
        response.set_header(Header::new(
            "Access-Control-Allow-Methods",
            "POST, GET, OPTIONS",
        ));
        response.set_header(Header::new("Access-Control-Allow-Headers", "*"));
        response.set_header(Header::new("Access-Control-Allow-Credentials", "true"));
    }
}

fn start_server(port: u16) {
    // launch server
    thread::spawn(move || {
        let mut config = rocket::config::Config::new(rocket::config::Environment::Production);
        config.set_port(port);
        // prevent multi thread access for easier sharing each cpu
        config.set_workers(1);
        config.set_log_level(rocket::logger::LoggingLevel::Off);

        // launch Restful
        rocket::custom(config)
            .attach(CORS)
            .mount(
                "/wasm",
                routes![instantiate_contract, execute_contract, query_contract],
            )
            .launch()
    });
}

fn query_wasm(request: &WasmQuery) -> QuerierResult {
    unsafe {
        match request {
            WasmQuery::Smart { contract_addr, msg } => {
                match ENGINES.get_mut(contract_addr.as_str()) {
                    None => SystemResult::Err(SystemError::NoSuchContract {
                        addr: contract_addr.to_owned(),
                    }),
                    Some(engine) => {
                        let result = cosmwasm_vm::call_query(
                            &mut engine.instance,
                            &engine.env,
                            msg.as_slice(),
                        );

                        // response can not unwrap, so it is empty
                        match result {
                            Ok(response) => SystemResult::Ok(response),
                            Err(err) => SystemResult::Err(SystemError::InvalidResponse {
                                error: err.to_string(),
                                response: Binary::from([]),
                            }),
                        }
                    }
                }
            }
            _ => SystemResult::Err(SystemError::UnsupportedRequest {
                kind: "Not implemented".to_string(),
            }),
        }
    }
}

fn check_is_need_slash(name: &str) -> bool {
    // Binary is base64 string input
    if name.eq("string") {
        return true;
    }
    return false;
}

fn to_json_item(name: &String, type_name: &str, engine: &ContractInstance) -> String {
    let (strip_type_name, optional) = match type_name.strip_suffix('?') {
        Some(s) => (s, true),
        None => (type_name, false),
    };
    let mut data: String = String::new();

    unsafe {
        EDITOR.readline(&mut data, true);
    }

    // do not append optional when empty
    if data.is_empty() && optional {
        return String::new();
    }

    // if Binary then first char is not the base64 then encode it to base64
    if strip_type_name.eq("Binary") && !data.chars().next().unwrap().is_alphabetic() {
        data = Binary::from(data.as_bytes()).to_base64();
    }

    let mapped_type_name = match engine.analyzer.map_of_basetype.get(strip_type_name) {
        None => strip_type_name,
        Some(v) => v,
    };
    let mut params = "\"".to_string();
    params.push_str(name);
    params.push_str("\":");
    if check_is_need_slash(mapped_type_name) {
        params.push('"');
        // clear enter and add slash to double quote
        params.push_str(data.replace('\n', "").replace('"', "\\\"").as_str());
        params.push('"');
    } else {
        params.push_str(data.as_str());
    }

    params.push(',');
    return params;
}

fn input_type(mem_name: &String, type_name: &String, engine: &ContractInstance) -> String {
    println!("input [{}]:", mem_name.blue().bold());
    let st = match engine.analyzer.map_of_struct.get_key_value(type_name) {
        Some(h) => h,
        _ => {
            // return to function, not return to st
            return to_json_item(&mem_name, &type_name, engine);
        }
    };
    //todo:need show all members by recursive invocation
    let mut params = "\"".to_string();
    params.push_str(mem_name);
    params.push_str("\":");

    if st.1.len() == 0 {
        unsafe {
            EDITOR.readline(&mut params, true);
        }
    } else {
        params.push('{');
        // member is default sorted
        for members in st.1 {
            println!(
                "input {}[{} : {}]:",
                INDENT,
                members.0.blue().bold(),
                members.1.yellow()
            );
            params.push_str(to_json_item(&members.0, members.1, engine).as_str());
        }
        // remove last , character
        if st.1.len() > 0 {
            params.pop();
        }
        params.push('}');
    }

    params.push(',');

    return params;
}

fn input_message(
    name: &str,
    members: &Vec<Member>,
    engine: &ContractInstance,
    is_enum: &bool,
) -> String {
    let mut final_msg: String = "{".to_string();

    if *is_enum {
        final_msg.push('"');
        final_msg.push_str(name);
        final_msg.push_str("\":{");
    }
    let mut option_values: String = String::new();
    for vcm in members {
        option_values
            .push_str(input_type(&vcm.member_name, &vcm.member_def.to_string(), engine).as_str());
    }

    // if there is option value then push to msg
    if option_values.len() > 0 {
        option_values.pop();
        final_msg.push_str(option_values.as_str());
    }

    final_msg.push('}');

    if *is_enum {
        final_msg.push('}');
    }

    final_msg
}

// get_call_type return value and indicate it is contract switch or account switch
fn get_call_type() -> Option<(String, bool, bool)> {
    let mut call_type = String::new();
    let mut params = vec![
        "instantiate".to_string(),
        "execute".to_string(),
        "query".to_string(),
    ];
    let mut contract_switch = false;
    let mut account_switch = false;

    print!(
        "Input call type ({} | {} | {}",
        "instantiate".green().bold(),
        "execute".green().bold(),
        "query".green().bold(),
    );
    unsafe {
        if ENGINES.len() > 1 {
            contract_switch = true;
        }
        if ACCOUNTS.len() > 1 {
            account_switch = true;
        }
        if contract_switch {
            print!(" | {}", "contract".blue().bold());
            params.push("contract".to_string());
        }
        if account_switch {
            print!(" | {}", "account".blue().bold());
            params.push("account".to_string());
        }

        // clone params to use contains without moving problem
        EDITOR.update_history_entries(params.clone());

        println!(")");

        EDITOR.readline(&mut call_type, false);

        if !params.contains(&call_type) {
            print!(
                "Wrong call type [{}], must one of ({} | {} | {}",
                call_type.red().bold(),
                "instantiate".green().bold(),
                "execute".green().bold(),
                "query".green().bold(),
            );
            if contract_switch {
                print!(" | {}", "contract".green().bold());
            }
            if account_switch {
                print!(" | {}", "contract".green().bold());
            }
            println!(")");
            return None;
        }

        // default messages
        if contract_switch && call_type.eq("contract") {
            let mut first = true;
            let mut call_param = String::new();
            print!("Choose smart contract [ ");

            EDITOR.clear_history();

            for k in sorted(ENGINES.keys()) {
                if first {
                    first = false;
                } else {
                    print!(" | ")
                }
                print!("{}", k.green().bold());
                EDITOR.add_history_entry(k);
            }

            print!(" ]\n");

            EDITOR.readline(&mut call_param, false);

            // check contract existed
            if ENGINES.get(&call_param).is_none() {
                println!("Smart contract {} not existed", call_param.red().bold());
                return None;
            }

            // return contract as switch param
            return Some((call_param, true, false));
        } else if account_switch && call_type.eq("account") {
            let mut first = true;
            let mut call_param = String::new();
            print!("Choose account [ ");

            EDITOR.clear_history();

            for info in ACCOUNTS.iter() {
                if first {
                    first = false;
                } else {
                    print!(" | ")
                }
                print!("{}", info.sender.as_str().green().bold());
                EDITOR.add_history_entry(info.sender.as_str());
            }

            print!(" ]\n");

            EDITOR.readline(&mut call_param, false);

            // check account existed
            if !ACCOUNTS
                .iter()
                .map(|x| x.sender.to_string())
                .collect::<Vec<String>>()
                .contains(&call_param)
            {
                println!("Account {} not existed", call_param.red().bold());
                return None;
            }

            // return contract as switch param
            return Some((call_param, false, true));
        }
    }

    return Some((call_type, false, false));
}

fn simulate_by_auto_analyze(
    engine: &mut ContractInstance,
    sender_addr: &str,
) -> Result<(bool, String, String), String> {
    // enable debug, show info
    if cfg!(debug_assertions) {
        engine.analyzer.dump_all_members();
        engine.analyzer.dump_all_definitions();
    }

    unsafe {
        let info = match ACCOUNTS.iter().find(|x| x.sender.as_str().eq(sender_addr)) {
            Some(i) => i,
            None => return Err(format!("No account found: {}", sender_addr)),
        };

        loop {
            println!(
                "Start_simulate with sender: {}, contract: {}, chain: {}, denom: {}, block height: {}",
                sender_addr.green().bold(),
                engine.env.contract.address.to_string().green().bold(),
                CHAIN_ID.green().bold(), DENOM.green().bold(), BLOCK_HEIGHT.to_string().green().bold()
            );

            let (call_type, contract_switch, account_switch) = match get_call_type() {
                None => continue,
                Some(s) => s,
            };

            let mut call_param = String::new();
            let mut first = true;
            // default messages
            if contract_switch {
                // change contract
                if engine.env.contract.address.to_string().ne(&call_type) {
                    return Ok((true, call_type, sender_addr.to_string()));
                }
                continue;
            } else if account_switch {
                // change account
                if sender_addr.ne(call_type.as_str()) {
                    return Ok((true, engine.env.contract.address.to_string(), call_type));
                }
                continue;
            } else if call_type.eq("instantiate")
                && engine.analyzer.map_of_member.contains_key("InstantiateMsg")
            {
                call_param = "InstantiateMsg".to_string();
            } else if call_type.eq("execute")
                && engine.analyzer.map_of_member.contains_key("ExecuteMsg")
            {
                call_param = "ExecuteMsg".to_string();
            } else if call_type.eq("query")
                && engine.analyzer.map_of_member.contains_key("QueryMsg")
            {
                call_param = "QueryMsg".to_string();
            } else {
                print!("Input Call param from [ ");

                EDITOR.clear_history();

                for k in sorted(engine.analyzer.map_of_member.keys()) {
                    if first {
                        first = false;
                    } else {
                        print!(" | ")
                    }
                    print!("{}", k.green().bold());
                    EDITOR.add_history_entry(k);
                }

                print!(" ]\n");

                EDITOR.readline(&mut call_param, false);
            }

            // if there is anyOf, it is enum
            let is_enum = engine
                .analyzer
                .map_of_enum
                .get(&call_param)
                .unwrap_or(&false);

            let msg_type: &HashMap<String, Vec<Member>> =
                match engine.analyzer.map_of_member.get(call_param.as_str()) {
                    None => {
                        println!("can not find msg type {}", call_param.as_str());
                        continue;
                    }
                    Some(v) => v,
                };
            let len = msg_type.len();
            if len > 0 {
                //only one msg
                if msg_type.len() == 1 {
                    call_param = msg_type.keys().next().unwrap().to_string();
                } else {
                    print!("Input Call param from [ ");
                    first = true;

                    EDITOR.clear_history();
                    for k in sorted(msg_type.keys()) {
                        if first {
                            first = false;
                        } else {
                            print!(" | ")
                        }
                        print!("{}", k.green().bold());
                        EDITOR.add_history_entry(k);
                    }

                    print!(" ]\n");
                    call_param.clear();

                    EDITOR.readline(&mut call_param, false);
                }
            }

            let json_msg = match msg_type.get(call_param.as_str()) {
                None => "{}".to_string(),
                Some(msg) => {
                    engine.analyzer.show_message_type(call_param.as_str(), msg);
                    input_message(call_param.as_str(), msg, engine, &is_enum)
                }
            };

            // update previous history entries
            EDITOR.update_input_history_entry();

            engine.call(call_type.as_str(), json_msg.as_str(), info);
        }
    }
}

fn simulate_by_json(
    engine: &mut ContractInstance,
    sender_addr: &str,
) -> Result<(bool, String, String), String> {
    unsafe {
        let info = match ACCOUNTS.iter().find(|x| x.sender.as_str().eq(sender_addr)) {
            Some(i) => i,
            None => return Err(format!("No account found: {}", sender_addr)),
        };

        loop {
            println!(
                "Start_simulate with sender: {}, contract: {}, chain: {}, denom: {}, block height: {}",
                sender_addr.green().bold(),
                engine.env.contract.address.to_string().green().bold(),
                CHAIN_ID.green().bold(), DENOM.green().bold(), BLOCK_HEIGHT.to_string().green().bold()
            );
            let (call_type, contract_switch, account_switch) = match get_call_type() {
                None => continue,
                Some(s) => s,
            };

            // default messages
            if contract_switch {
                if engine.env.contract.address.to_string().ne(&call_type) {
                    return Ok((true, call_type, sender_addr.to_string()));
                }
                continue;
            } else if account_switch {
                // change account
                if sender_addr.ne(call_type.as_str()) {
                    return Ok((true, engine.env.contract.address.to_string(), call_type));
                }
                continue;
            }

            println!("Input json string:");
            let mut json_msg = String::new();
            // update previous history entries

            EDITOR.update_input_history_entry();
            EDITOR.readline(&mut json_msg, true);

            engine.call(call_type.as_str(), json_msg.as_str(), info);
        }
    }
}

// start_simulate will return next contract and account to run
fn start_simulate(
    contract_addr: &str,
    sender_addr: &str,
) -> Result<(bool, String, String), String> {
    unsafe {
        match ENGINES.get_mut(contract_addr) {
            Some(engine) => {
                // enable debug
                if cfg!(debug_assertions) {
                    engine.show_module_info();
                }
                if engine.analyzer.map_of_member.is_empty() {
                    simulate_by_json(engine, sender_addr)
                } else {
                    simulate_by_auto_analyze(engine, sender_addr)
                }
            }
            None => Err(format!("No engine found: {}", contract_addr)),
        }
    }
}

fn start_simulate_forever(contract_addr: &str, sender_addr: &str) -> bool {
    match start_simulate(contract_addr, sender_addr) {
        Ok(ret) => {
            if ret.0 {
                // recursive
                start_simulate_forever(ret.1.as_str(), ret.2.as_str())
            } else {
                println!(
                    "start_simulate failed for contract: {}",
                    contract_addr.blue().bold()
                );
                return false;
            }
        }
        Err(e) => {
            println!("error occurred during call start_simulate : {}", e.red());
            return false;
        }
    }
}

fn load_artifacts(
    file_path: &str,
    contract_folder: Option<&str>,
) -> Result<Vec<(String, String)>, Error> {
    // check file
    if !file_path.ends_with(".wasm") {
        println!(
            "only support file[*.wasm], you just input a wrong file format - {}",
            file_path.green().bold()
        );
        return Err(Error::new(ErrorKind::InvalidInput, file_path));
    }
    let wasm_file = Path::new(file_path);
    if !wasm_file.is_file() {
        return Err(Error::new(ErrorKind::NotFound, file_path));
    }

    let contract_addr = wasm_file.file_stem().unwrap().to_str().unwrap();
    let mut file_paths = vec![(file_path.to_string(), contract_addr.to_string())];

    let seg = match file_path.rfind('/') {
        None => return Ok(file_paths),
        Some(idx) => idx,
    };
    let (parent_path, _) = file_path.split_at(seg);

    if let Some(folder) = contract_folder {
        let artifacts_folder = std::path::Path::new(parent_path).join(folder);

        if artifacts_folder.is_dir() {
            for entry in std::fs::read_dir(artifacts_folder)? {
                let dir = entry?;
                if let Some(contract_addr) = dir.file_name().to_str() {
                    if let Some(file) = dir.path().to_str() {
                        let wasm_file = Path::new(file).join(format!("{}.wasm", contract_addr));
                        if wasm_file.is_file() {
                            file_paths.push((
                                wasm_file.to_str().unwrap().to_string(),
                                contract_addr.to_string(),
                            ));
                        }
                    }
                }
            }
        }
    }

    Ok(file_paths)
}

// execute_contract_response currently support execute only, with new message info from send fund param
fn execute_contract_response(sender_addr: &str, messages: Vec<CosmosMsg>) -> Vec<Attribute> {
    let mut attributes: Vec<Attribute> = vec![];
    unsafe {
        for msg in messages {
            // only clone required properties
            if let CosmosMsg::Wasm(WasmMsg::Execute {
                contract_addr,
                msg,
                send,
            }) = &msg
            {
                let result = match ENGINES.get_mut(contract_addr.as_str()) {
                    None => format!("No such contract: {}", contract_addr),
                    Some(engine) => {
                        engine.execute_raw(
                            msg.as_slice(),
                            &MessageInfo {
                                sender: Addr::unchecked(sender_addr),
                                // there is default account with balance
                                funds: send.clone(),
                            },
                        )
                    }
                };
                attributes.push(Attribute {
                    key: contract_addr.to_string(),
                    value: result,
                })
            }
        }
    }

    attributes
}

fn insert_engine(
    wasm_file: &str,
    contract_addr: &str,
    wasm_handler: WasmHandler,
    storage: &MockStorage,
) {
    match ContractInstance::new_instance(
        wasm_file,
        contract_addr,
        wasm_handler,
        storage,
        execute_contract_response,
    ) {
        Err(e) => {
            println!("error occurred during install contract: {}", e.red());
        }
        Ok(engine) => {
            unsafe { ENGINES.insert(contract_addr.to_owned(), engine) };
        }
    };
}

fn watch_and_update(
    sender: &sync::mpsc::Sender<String>,
    wasm_files: &Vec<(String, String)>,
) -> Result<bool, Error> {
    // do not copy, use reference when loop
    let len = wasm_files.len();
    let mut modified_files: Vec<time::SystemTime> = vec![time::SystemTime::now(); len];

    loop {
        for index in 0..len {
            let (wasm_file, contract_addr) = &wasm_files[index];

            if let Ok(modified_time) = fs::metadata(wasm_file)?.modified() {
                if modified_time.eq(&modified_files[index]) {
                    continue;
                }
                modified_files[index] = modified_time;
            }

            unsafe {
                match ENGINES.get_mut(contract_addr) {
                    Some(eng) => {
                        // sleep 100 miliseconds incase it notifies modification before build version is completed
                        thread::sleep(time::Duration::from_millis(100));
                        // callback query directly from storage to copy it
                        eng.instance
                            .with_storage(|storage| {
                                insert_engine(wasm_file, contract_addr, query_wasm, storage);
                                Ok(())
                            })
                            .unwrap();
                    }
                    None => {
                        insert_engine(
                            wasm_file,
                            contract_addr,
                            query_wasm,
                            &contract_vm::mock::MockStorage::default(),
                        );
                    }
                };
            }
        }

        // init all contracts first time, send the first contract to notify
        if len > 0 {
            sender.send(wasm_files[0].1.to_owned()).unwrap();
        }

        // watch again every second
        thread::sleep(time::Duration::from_millis(1000));
    }
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
pub struct CointBalance {
    pub address: Addr,
    pub amount: Uint128,
}

fn prepare_command_line() -> bool {
    let matches = App::new("cosmwasm-simulate")
        .version("0.1.0")
        .author("github : https://github.com/oraichain/cosmwasm-simulate.git")
        .about("A simulation of cosmwasm smart contract system")
        .arg(
            Arg::with_name("run")
                .help("contract file that built by https://github.com/oraichain/smart-studio.git")
                .empty_values(false),
        )
        .arg(Arg::with_name("port").help("port of restful server"))
        .arg(Arg::from_usage(
            "-c, --contract=[CONTRACT_FOLDER] 'Other contract folder'",
        ))
        .arg(
            Arg::from_usage("-b, --balance=[COIN_BALANCE] 'Other coin balance, multiple'")
                .multiple(true),
        )
        .get_matches();

    unsafe {
        ACCOUNTS.push(MessageInfo {
            sender: Addr::unchecked(DEFAULT_SENDER_ADDR),
            // there is default account with balance
            funds: vec![Coin {
                denom: DENOM.to_string(),
                amount: Uint128::from(DEFAULT_SENDER_BALANCE),
            }],
        });

        // add more balances
        if let Some(coin_balances) = matches.values_of("balance") {
            for file in coin_balances.collect::<Vec<&str>>() {
                let coin_balance: CointBalance = from_slice(file.as_bytes()).unwrap();
                // add sent_funds if not zero
                let sent_funds = match coin_balance.amount.is_zero() {
                    true => vec![],
                    false => vec![Coin {
                        denom: DENOM.to_string(),
                        amount: coin_balance.amount,
                    }],
                };
                ACCOUNTS.push(MessageInfo {
                    sender: coin_balance.address,
                    funds: sent_funds,
                });
            }
        }

        // Sort by sender address
        ACCOUNTS.sort_by(|a, b| a.sender.cmp(&b.sender));
    }

    if let Some(port_str) = matches.value_of("port") {
        if let Ok(port) = port_str.parse() {
            let debug = match std::env::var("DEBUG") {
                Ok(s) => s.ne("false"),
                _ => false,
            };

            if debug {
                println!("🚀 Rest API Base URL: http://0.0.0.0:{}/wasm", port);
            }

            start_server(port);
        }
    }

    if let Some(file) = matches.value_of("run") {
        // start load, check other file as well
        let wasm_files = match load_artifacts(file, matches.value_of("contract")) {
            Err(_) => vec![],
            Ok(s) => s,
        };

        let (sender, receiver) = sync::mpsc::channel();
        // Spawn off an expensive computation
        thread::spawn(move || {
            // trigger init lazy mut
            unsafe {
                ENGINES.init_once();
            }

            if let Ok(ret) = watch_and_update(&sender, &wasm_files) {
                return ret;
            }
            return true;
        });

        // simulate until break, start with first contract
        match receiver.recv() {
            Ok(contract_addr) => {
                unsafe {
                    // init the first suggested items
                    for k in ACCOUNTS.iter() {
                        EDITOR.add_input_history_entry(k.sender.to_string());
                    }
                    for k in ENGINES.keys() {
                        EDITOR.add_input_history_entry(k.to_owned());
                    }
                }
                return start_simulate_forever(contract_addr.as_str(), DEFAULT_SENDER_ADDR);
            }
            Err(e) => {
                println!("watch error: {}", e.to_string().red());
                return false;
            }
        }
    }
    return false;
}

fn main() {
    prepare_command_line();
}
