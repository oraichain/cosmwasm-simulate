#![feature(proc_macro_hygiene, decl_macro)]

extern crate base64;
extern crate clap;

pub mod contract_vm;

use crate::contract_vm::analyzer::{Member, INDENT};
use crate::contract_vm::editor::TerminalEditor;
use crate::contract_vm::engine::ContractInstance;
use clap::{App, Arg};
use colored::*;
use cosmwasm_std::{QuerierResult, SystemError, SystemResult, WasmQuery};
use rocket::response::content;
use std::collections::HashMap;
use std::io::{Error, ErrorKind};
use std::ops::Add;
use std::path::Path;
use std::{fs, sync, thread, time};

// default const is 'static lifetime
const CONTRACT_FOLDER: &str = "contract";
const SENDER_ADDR: &str = "fake_sender_addr";

#[macro_use]
extern crate lazy_mut;
lazy_mut! {
    static mut EDITOR: TerminalEditor = TerminalEditor::new();
    static mut ENGINES : HashMap<String, ContractInstance> = HashMap::new();
}

#[macro_use]
extern crate rocket;

fn call_engine(contract_addr: &str, func_type: &str, msg: &str) -> Result<String, String> {
    unsafe {
        match ENGINES.get_mut(contract_addr) {
            None => Err(format!("No such contract: {}", contract_addr)),
            Some(engine) => match base64::decode(msg.as_bytes()) {
                Ok(input) => match String::from_utf8(input) {
                    Ok(param) => Ok(engine.call(func_type.to_owned(), param).to_owned()),
                    Err(err) => Err(err.to_string()),
                },
                Err(err) => Err(err.to_string()),
            },
        }
    }
}

#[get("/contract/<address>/init/<msg>")]
fn init_contract(address: String, msg: String) -> content::Json<String> {
    match call_engine(address.as_str(), "init", msg.as_str()) {
        Ok(data) => content::Json(format!(r#"{{"data": {}}}"#, data)),
        Err(err) => content::Json(format!(r#"{{"error": "{}"}}"#, err)),
    }
}

#[get("/contract/<address>/handle/<msg>")]
fn handle_contract(address: String, msg: String) -> content::Json<String> {
    match call_engine(address.as_str(), "handle", msg.as_str()) {
        Ok(data) => content::Json(format!(r#"{{"data": {}}}"#, data)),
        Err(err) => content::Json(format!(r#"{{"error": "{}"}}"#, err)),
    }
}

#[get("/contract/<address>/query/<msg>")]
fn query_contract(address: String, msg: String) -> content::Json<String> {
    match call_engine(address.as_str(), "query", msg.as_str()) {
        Ok(data) => content::Json(format!(r#"{{"data": {}}}"#, data)),
        Err(err) => content::Json(format!(r#"{{"error": "{}"}}"#, err)),
    }
}

fn start_server() {
    // launch server
    thread::spawn(move || {
        // launch Restful
        rocket::ignite()
            .mount(
                "/wasm",
                routes![init_contract, handle_contract, query_contract],
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
                        )
                        .unwrap();
                        SystemResult::Ok(result)
                    }
                }
            }
            _ => SystemResult::Err(SystemError::UnsupportedRequest {
                kind: "Not implemented".to_string(),
            }),
        }
    }
}

fn input_with_out_handle(input_data: &mut String, store_input: bool) -> bool {
    unsafe { EDITOR.readline(input_data, store_input) }
}

fn check_is_need_slash(name: &str) -> bool {
    if name == "string" {
        return true;
    }
    return false;
}

fn to_json_item(name: &String, type_name: &str, engine: &ContractInstance) -> String {
    let mut data: String = String::new();
    input_with_out_handle(&mut data, true);
    let mapped_type_name = match engine.analyzer.map_of_basetype.get(type_name) {
        None => type_name,
        Some(v) => v,
    };
    let mut params = "\"".to_string();
    params += name.as_str();
    params += "\":";
    if check_is_need_slash(mapped_type_name) {
        params += "\"";
        // clear enter and add slash to double quote
        params += data.replace('\n', "").replace('"', "\\\"").as_str();
        params += "\"";
    } else {
        params += data.as_str();
    }

    params += ",";
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
    params += mem_name;
    params += "\":";

    if st.1.len() == 0 {
        input_with_out_handle(&mut params, true);
    } else {
        params += "{";
        for members in st.1 {
            println!(
                "input {}[{} : {}]:",
                INDENT,
                members.0.blue().bold(),
                members.1.yellow()
            );
            params += to_json_item(&members.0, members.1, engine).as_str();
        }
        let (resv, _) = params.split_at(params.len() - 1);
        params = resv.to_string();
        params += "}";
    }

    params += ",";

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
        final_msg = final_msg.add("\"");
        final_msg = final_msg.add(name);
        final_msg = final_msg.add("\":{");
    }

    for vcm in members {
        final_msg = final_msg
            .add(input_type(&vcm.member_name, &vcm.member_def.to_string(), engine).as_str());
    }
    if members.len() > 0 {
        let (resv, _) = final_msg.split_at(final_msg.len() - 1);
        final_msg = resv.to_string();
        final_msg = final_msg.add("}");
    } else {
        final_msg = final_msg.add("}");
    }

    if *is_enum {
        final_msg = final_msg.add("}");
    }
    return final_msg;
}

fn get_call_type() -> Option<(String, bool)> {
    let mut call_type = String::new();
    let mut params = vec![
        "init".to_string(),
        "handle".to_string(),
        "query".to_string(),
    ];
    let mut need_switch = false;

    print!(
        "Input call type ({} | {} | {}",
        "init".green().bold(),
        "handle".green().bold(),
        "query".green().bold(),
    );
    unsafe {
        if ENGINES.len() > 1 {
            need_switch = true;
        }
        if need_switch {
            print!(" | {}", "switch".blue().bold());
            params.push("switch".to_string());
        }
        EDITOR.update_history_entries(params);
    }
    println!(")");
    input_with_out_handle(&mut call_type, false);
    if call_type.ne("init")
        && call_type.ne("handle")
        && call_type.ne("query")
        && need_switch
        && call_type.ne("switch")
    {
        print!(
            "Wrong call type [{}], must one of ({} | {} | {}",
            call_type.red().bold(),
            "init".green().bold(),
            "handle".green().bold(),
            "query".green().bold(),
        );
        if need_switch {
            print!(" | {}", "switch".green().bold());
        }
        println!(")");
        return None;
    }

    // default messages
    if need_switch && call_type.eq("switch") {
        let mut first = true;
        let mut call_param = String::new();
        print!("Choose smart contract [ ");
        unsafe {
            EDITOR.clear_history();

            for k in ENGINES.keys() {
                if first {
                    first = false;
                } else {
                    print!(" | ")
                }
                print!("{}", k.green().bold());
                EDITOR.add_history_entry(k);
            }
        }
        print!(" ]\n");

        input_with_out_handle(&mut call_param, false);

        // check contract existed
        unsafe {
            if ENGINES.get(&call_param).is_none() {
                println!("Smart contract {} not existed", call_param.red().bold());
                return None;
            }
        }

        // return contract as switch param
        return Some((call_param, true));
    }

    return Some((call_type, false));
}

fn simulate_by_auto_analyze(
    engine: &mut ContractInstance,
    sender_addr: &str,
) -> Result<(bool, String), String> {
    // enable debug, show info
    if cfg!(debug_assertions) {
        engine.analyzer.dump_all_members();
        engine.analyzer.dump_all_definitions();
    }

    loop {
        println!(
            "Start_simulate with sender: {}, contract: {}",
            sender_addr.green().bold(),
            engine.env.contract.address.green().bold()
        );

        let (call_type, is_switch) = match get_call_type() {
            None => continue,
            Some(s) => s,
        };

        let mut call_param = String::new();
        let mut first = true;
        // default messages
        if is_switch {
            if engine.env.contract.address.to_string().ne(&call_param) {
                return Ok((true, call_type));
            }
            continue;
        } else if call_type.eq("init") && engine.analyzer.map_of_member.contains_key("InitMsg") {
            call_param = "InitMsg".to_string();
        } else if call_type.eq("handle") && engine.analyzer.map_of_member.contains_key("HandleMsg")
        {
            call_param = "HandleMsg".to_string();
        } else if call_type.eq("query") && engine.analyzer.map_of_member.contains_key("QueryMsg") {
            call_param = "QueryMsg".to_string();
        } else {
            print!("Input Call param from [ ");

            unsafe {
                EDITOR.clear_history();

                for k in engine.analyzer.map_of_member.keys() {
                    if first {
                        first = false;
                    } else {
                        print!(" | ")
                    }
                    print!("{}", k.green().bold());
                    EDITOR.add_history_entry(k);
                }
            }

            print!(" ]\n");

            input_with_out_handle(&mut call_param, false);
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

                unsafe {
                    EDITOR.clear_history();
                    for k in msg_type.keys() {
                        if first {
                            first = false;
                        } else {
                            print!(" | ")
                        }
                        print!("{}", k.green().bold());
                        EDITOR.add_history_entry(k);
                    }
                }

                print!(" ]\n");
                call_param.clear();
                input_with_out_handle(&mut call_param, false);
            }
        }

        let msg = match msg_type.get(call_param.as_str()) {
            None => {
                println!("can not find msg type {}", call_param.as_str());
                for k in msg_type.keys() {
                    print!("{}", k);
                }
                continue;
            }
            Some(v) => v,
        };

        engine.analyzer.show_message_type(call_param.as_str(), msg);

        // update previous history entries
        unsafe {
            EDITOR.update_input_history_entry();
        }

        let json_msg = input_message(call_param.as_str(), msg, engine, &is_enum);

        println!("call {} - {}", call_type, json_msg);

        engine.call(call_type, json_msg);
    }
}

fn simulate_by_json(
    engine: &mut ContractInstance,
    sender_addr: &str,
) -> Result<(bool, String), String> {
    loop {
        println!(
            "Start_simulate with sender: {}, contract: {}",
            sender_addr.green().bold(),
            engine.env.contract.address.green().bold()
        );
        let (call_type, is_switch) = match get_call_type() {
            None => continue,
            Some(s) => s,
        };

        // default messages
        if is_switch {
            if engine.env.contract.address.to_string().ne(&call_type) {
                return Ok((true, call_type));
            }
            continue;
        }

        println!("Input json string:");
        let mut json_msg = String::new();
        // update previous history entries
        unsafe {
            EDITOR.update_input_history_entry();
        }

        input_with_out_handle(&mut json_msg, true);
        engine.call(call_type, json_msg);
    }
}

fn start_simulate(contract_addr: &str, sender_addr: &str) -> Result<(bool, String), String> {
    unsafe {
        let engine = ENGINES.get_mut(contract_addr).unwrap();
        // enable debug
        if cfg!(debug_assertions) {
            engine.show_module_info();
        }
        if engine.analyzer.map_of_member.is_empty() {
            return simulate_by_json(engine, sender_addr);
        } else {
            return simulate_by_auto_analyze(engine, sender_addr);
        }
    }
}

fn start_simulate_forever(contract_addr: &str, sender_addr: &str) -> bool {
    match start_simulate(contract_addr, sender_addr) {
        Ok(ret) => {
            if ret.0 {
                println!(
                    "start_simulate success for contract: {}",
                    contract_addr.blue().bold()
                );
                // recursive
                start_simulate_forever(ret.1.as_str(), sender_addr)
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

fn load_artifacts(file_path: &str) -> Result<Vec<(String, String)>, Error> {
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

    let artifacts_folder = std::path::Path::new(parent_path).join(CONTRACT_FOLDER);

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

    Ok(file_paths)
}

fn watch_and_update(
    sender: &sync::mpsc::Sender<String>,
    sender_addr: &str,
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

            match contract_vm::build_simulation(
                wasm_file.as_str(),
                contract_addr.as_str(),
                sender_addr,
                query_wasm,
            ) {
                Err(e) => {
                    println!("error occurred during install contract: {}", e.red());
                    return Err(Error::new(ErrorKind::Other, e));
                }
                Ok(instance) => unsafe {
                    // println!("installed contract: {}", contract_addr.blue().bold());
                    ENGINES.insert(contract_addr.to_string(), instance);
                },
            };
        }

        // init all contracts first time, send the first contract to notify
        if len > 0 {
            sender.send(wasm_files[0].1.to_owned()).unwrap();
        }

        thread::sleep(time::Duration::from_millis(100));
    }
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
        .get_matches();

    if let Some(port) = matches.value_of("port") {
        std::env::set_var("ROCKET_PORT", port);
        start_server();
    }

    if let Some(file) = matches.value_of("run") {
        // start load, check other file as well
        let wasm_files = match load_artifacts(file) {
            Err(_) => vec![],
            Ok(s) => s,
        };

        let (sender, receiver) = sync::mpsc::channel();
        // Spawn off an expensive computation
        thread::spawn(move || {
            if let Ok(ret) = watch_and_update(&sender, SENDER_ADDR, &wasm_files) {
                return ret;
            }
            return true;
        });

        // simulate until break, start with first contract
        match receiver.recv() {
            Ok(contract_addr) => {
                return start_simulate_forever(contract_addr.as_str(), SENDER_ADDR);
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
