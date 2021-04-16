extern crate clap;

pub mod contract_vm;

use crate::analyzer::INDENT;
use crate::contract_vm::analyzer::Member;
use crate::contract_vm::editor::TerminalEditor;
use crate::contract_vm::engine::ContractInstance;
use clap::{App, Arg};
use colored::*;
use contract_vm::analyzer;
use std::collections::HashMap;
use std::ops::Add;
use std::path::Path;

#[macro_use]
extern crate lazy_mut;
lazy_mut! {
    static mut EDITOR: TerminalEditor = TerminalEditor::new();
    static mut ENGINES : HashMap<String, ContractInstance> = HashMap::new();
}

fn input_with_out_handle(input_data: &mut String, store_input: bool) -> bool {
    unsafe { EDITOR.readline(input_data, store_input) }
}

fn show_message_type(name: &str, members: &Vec<Member>, engine: &ContractInstance) {
    println!("{} {{", name.blue().bold());
    for vcm in members {
        let st = match engine
            .analyzer
            .map_of_struct
            .get_key_value(vcm.member_def.as_str())
        {
            Some(h) => h,
            _ => {
                println!(
                    "{}{} : {}",
                    INDENT,
                    vcm.member_name.blue().bold(),
                    vcm.member_def.yellow()
                );
                continue;
            }
        };
        //todo:need show all members by recursive invocation
        println!(
            "{}{} : {} {{ ",
            INDENT,
            vcm.member_name.blue().bold(),
            vcm.member_def.yellow()
        );
        for members in st.1 {
            println!(
                "{}{} : {}",
                INDENT.repeat(2),
                members.0.blue().bold(),
                members.1.yellow()
            );
        }
        println!("{}}}", INDENT);
    }
    println!("}}");
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

fn simulate_by_auto_analyze(engine: &mut ContractInstance, contract_addr: &str) {
    loop {
        println!(
            "Start_simulate with sender address: {}",
            contract_addr.green().bold()
        );
        let mut call_type = String::new();
        let mut call_param = String::new();
        println!(
            "Input call type ({} | {} | {}):",
            "init".green().bold(),
            "handle".green().bold(),
            "query".green().bold()
        );
        unsafe {
            EDITOR.update_history_entries(vec![
                "init".to_string(),
                "handle".to_string(),
                "query".to_string(),
            ]);
        }
        input_with_out_handle(&mut call_type, false);
        if call_type.ne("init") && call_type.ne("handle") && call_type.ne("query") {
            println!(
                "Wrong call type[{}], must one of (init | handle | query)",
                call_type
            );
            continue;
        }

        let mut first = true;
        // default messages
        if call_type.eq("init") && engine.analyzer.map_of_member.contains_key("InitMsg") {
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

        show_message_type(call_param.as_str(), msg, &engine);

        // update previous history entries
        unsafe {
            EDITOR.update_input_history_entry();
        }
        let json_msg = input_message(call_param.as_str(), msg, &engine, &is_enum);

        println!("call {} - {}", call_type, json_msg);

        let result = engine.call(call_type, json_msg);
        println!("Call return msg [{}]", result);
    }
}

fn simulate_by_json(engine: &mut ContractInstance, contract_addr: &str) {
    loop {
        println!(
            "Start_simulate with sender address: {}",
            contract_addr.green().bold()
        );
        let mut call_type = String::new();
        let mut json_msg = String::new();
        println!(
            "Input call type ({} | {} | {}):",
            "init".green().bold(),
            "handle".green().bold(),
            "query".green().bold()
        );

        unsafe {
            EDITOR.update_history_entries(vec![
                "init".to_string(),
                "handle".to_string(),
                "query".to_string(),
            ]);
        }
        input_with_out_handle(&mut call_type, false);
        if call_type.ne("init") && call_type.ne("handle") && call_type.ne("query") {
            println!(
                "Wrong call type[{}], must one of (init | handle | query)",
                call_type
            );
            continue;
        }
        println!("Input json string:");

        // update previous history entries
        unsafe {
            EDITOR.update_input_history_entry();
        }
        input_with_out_handle(&mut json_msg, true);
        let result = engine.call(call_type, json_msg);
        println!("Call return msg [{}]", result);
    }
}

fn start_simulate(wasmfile: &str, sender_addr: &str) -> Result<bool, String> {
    let contract_addr = Path::new(wasmfile).file_stem().unwrap().to_str().unwrap();
    unsafe {
        let mut engine = ENGINES.get_mut(contract_addr).unwrap();
        // enable debug
        if cfg!(debug_assertions) {
            engine.show_module_info();
        }
        if engine.analyzer.auto_load_json_schema(&engine.wasm_file) {
            // show info
            engine.analyzer.dump_all_members();
            engine.analyzer.dump_all_definitions();

            simulate_by_auto_analyze(&mut engine, sender_addr);
        } else {
            simulate_by_json(&mut engine, sender_addr);
        }
    }
    return Ok(true);
}

fn prepare_command_line() -> bool {
    let matches = App::new("cosmwasm-simulate")
        .version("0.1.0")
        .author("github : https://github.com/oraichain/cosmwasm-simulate.git")
        .about("A simulation of cosmwasm smart contract system")
        .arg(
            Arg::with_name("run")
                .help("contract file that built by https://github.com/oraichain/smart-studio")
                .empty_values(false),
        )
        .arg(
            Arg::with_name("sender")
                .help("Sender Address")
                .default_value("fake_sender_addr"),
        )
        .get_matches();

    if let Some(file) = matches.value_of("run") {
        if !file.ends_with(".wasm") {
            println!(
                "only support file[*.wasm], you just input a wrong file format - {:?}",
                file
            );
            return false;
        }

        // start load
        // let mut engine = match contract_vm::build_simulation(wasmfile, contract_addr, sender_addr) {
        //     Err(e) => return Err(e),
        //     Ok(instance) => instance,
        // };

        if let Some(sender_addr) = matches.value_of("sender") {
            match start_simulate(file, sender_addr) {
                Ok(t) => {
                    if t {
                        println!("start_simulate success");
                    } else {
                        println!("start_simulate failed")
                    }
                }
                Err(e) => println!("error occurred during call start_simulate : {}", e),
            }
            return true;
        }
    }
    return false;
}

fn main() {
    prepare_command_line();
}
