//analyzer for json schema file

use colored::*;
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;

pub const INDENT: &str = "    ";

fn get_member_name_from_definition(item: &str) -> &str {
    match item.rfind('/') {
        None => item,
        Some(idx) => {
            let (_, short_name) = item.split_at(idx + 1);
            short_name
        }
    }
}

fn get_type_name_from_definition(item: &serde_json::Value) -> &serde_json::Value {
    match item.get("type") {
        None => {
            // check if all of type
            let item = match item.get("allOf") {
                None => item,
                Some(v) => v.as_array().unwrap().first().unwrap(),
            };

            match item.get("$ref") {
                // default is Null
                None => &serde_json::Value::Null,
                Some(rf) => rf,
            }
        }
        Some(t) => t,
    }
}

//Todo: analyze more detail from json schema file
pub struct StructType {
    pub member_name: String,
    pub member_type: String,
}

pub struct Member {
    pub member_name: String,
    pub member_def: String,
}

pub struct Analyzer {
    pub map_of_basetype: HashMap<String, String>,
    pub map_of_struct: HashMap<String, HashMap<String, String>>,
    pub map_of_member: HashMap<String, HashMap<String, Vec<Member>>>,
    pub map_of_enum: HashMap<String, bool>,
}

impl Analyzer {
    pub fn default() -> Self {
        return Analyzer {
            map_of_basetype: HashMap::new(),
            map_of_struct: HashMap::new(),
            map_of_member: HashMap::new(),
            map_of_enum: HashMap::new(),
        };
    }

    pub fn build_member(
        required: &serde_json::Value,
        properties: &serde_json::Value,
        mem_name: &String,
        struct_type: &HashMap<String, HashMap<String, String>>,
        mapper: &mut HashMap<String, Vec<Member>>,
    ) -> bool {
        let req_arr = match required.as_array() {
            None => vec![],
            Some(arr) => arr.to_vec(),
        };

        mapper.insert(mem_name.clone(), Vec::new());

        let vec_mem = match mapper.get_mut(mem_name) {
            None => return false,
            Some(vecm) => vecm,
        };

        if req_arr.len() == 0 {
            let type_name = match properties.get("$ref") {
                None => return false,
                Some(def_str) => get_member_name_from_definition(def_str.as_str().unwrap()),
            };

            match struct_type.get(type_name) {
                None => return false,
                Some(st) => {
                    for members in st {
                        let member: Member = Member {
                            member_name: members.0.to_string(),
                            member_def: members.1.to_string(),
                        };
                        vec_mem.insert(vec_mem.len(), member);
                    }
                }
            };
        } else {
            for req in req_arr {
                let req_str = match req.as_str() {
                    None => continue,
                    Some(s) => s,
                };
                let proper = match properties.get(req_str) {
                    None => continue,
                    Some(ps) => ps,
                };
                let type_name = get_type_name_from_definition(proper);
                let name = match type_name.as_str() {
                    None => continue,
                    Some(s) => s,
                };
                let mut member: Member = Member {
                    member_name: req_str.to_string(),
                    member_def: "".to_string(),
                };

                // not support input array of Definition, it is difficult to process
                if name == "array" {
                    let item = match proper.get("items") {
                        None => continue,
                        Some(it) => match it.get("$ref") {
                            // get type directly
                            None => match it.get("type") {
                                None => continue,
                                Some(t) => match t.as_str() {
                                    None => continue,
                                    Some(s) => s,
                                },
                            },
                            Some(rf) => match rf.as_str() {
                                None => continue,
                                Some(s) => s,
                            },
                        },
                    };
                    // array types
                    member.member_def = format!("[{}]", get_member_name_from_definition(item));
                } else if name.starts_with("#/definitions") {
                    // struct
                    member.member_def = get_member_name_from_definition(name).to_string();
                } else {
                    //base type
                    member.member_def = name.to_string();
                }
                vec_mem.insert(vec_mem.len(), member);
            }
        }
        return true;
    }

    pub fn dump_all_definitions(&self) {
        if self.map_of_basetype.len() > 0 {
            println!("{}", "Base Type :".green().bold());
            for b in &self.map_of_basetype {
                println!("{}{} => {}", INDENT, b.0.blue().bold(), b.1.yellow());
            }
        }
        if self.map_of_struct.len() > 0 {
            println!("{}", "Struct Type :".green().bold());
            for s in &self.map_of_struct {
                println!("{}{} {{", INDENT, s.0.blue().bold());
                for member in s.1 {
                    println!(
                        "{}{} : {}",
                        INDENT.repeat(2),
                        member.0.blue().bold(),
                        member.1.yellow()
                    );
                }
                println!("{}}}", INDENT);
            }
        }
    }

    pub fn dump_all_members(&self) {
        for b in &self.map_of_member {
            let is_enum = self.map_of_enum.get(b.0).unwrap_or(&false);
            let mut tab = "";
            if *is_enum {
                println!("{} {{", b.0.blue().bold());
                tab = INDENT;
            }
            for vcm in b.1 {
                if vcm.1.len() > 0 {
                    println!("{}{} {{", tab, vcm.0.blue().bold());
                    for vc in vcm.1 {
                        println!(
                            "{}{}{} : {}",
                            tab,
                            INDENT,
                            vc.member_name.blue().bold(),
                            vc.member_def.yellow()
                        );
                    }
                    println!("{}}}", tab);
                } else {
                    println!("{}{} {{ }}", tab, vcm.0.blue().bold());
                }
            }
            if *is_enum {
                println!("}}")
            }
        }
    }

    pub fn prepare_definitions(
        def: &serde_json::Value,
        base_type: &mut HashMap<String, String>,
        struct_type: &mut HashMap<String, HashMap<String, String>>,
    ) -> bool {
        let mut vec_struct: HashMap<String, String> = HashMap::new();
        let def_arr = match def.as_object() {
            None => return false,
            Some(da) => da,
        };

        for d in def_arr {
            let type_def = get_type_name_from_definition(d.1);

            if type_def == "object" {
                //struct
                let prop = match d.1.get("properties") {
                    None => continue,
                    Some(p) => p,
                };

                let prop_map = match prop.as_object() {
                    None => continue,
                    Some(pm) => pm,
                };

                for p in prop_map {
                    // recursive parse
                    let type_str = get_type_name_from_definition(p.1);

                    let def_str = match type_str.as_array() {
                        None => type_str.as_str().unwrap_or_default(),
                        Some(s) => s.first().unwrap().as_str().unwrap_or_default(),
                    };

                    // check if is definition type
                    let short_name = get_member_name_from_definition(def_str);
                    vec_struct.insert(p.0.to_string(), short_name.to_string());
                }
                struct_type.insert(d.0.to_string(), vec_struct.clone());
            } else {
                //base type
                let def = match type_def.as_str() {
                    None => continue,
                    Some(s) => s,
                };
                base_type.insert("".to_string() + d.0, def.to_string());
            }
        }
        return true;
    }

    fn analyze_schema(&mut self, path: String) -> bool {
        let data = match load_data_from_file(path.as_str()) {
            Err(_e) => return false,
            Ok(code) => code,
        };
        let translated: serde_json::Value = match serde_json::from_slice(data.as_slice()) {
            Ok(trs) => trs,
            Err(_e) => return false,
        };
        let title_must_exist = match translated["title"].as_str() {
            None => return false,
            Some(title) => title,
        };

        let mapping = match translated.as_object() {
            None => return false,
            Some(kvs) => kvs,
        };

        self.map_of_member
            .insert(title_must_exist.to_string(), HashMap::new());
        let mut current_member = match self.map_of_member.get_mut(&title_must_exist.to_string()) {
            None => return false,
            Some(c) => c,
        };
        // prepare definitions before analyzing
        for iter in mapping.iter() {
            if iter.0 == "definitions" {
                Analyzer::prepare_definitions(
                    &iter.1,
                    &mut self.map_of_basetype,
                    &mut self.map_of_struct,
                );
            }
        }
        // process other fields
        for iter in mapping.iter() {
            if iter.0 == "required" {
                let properties = match mapping.get("properties") {
                    None => continue,
                    Some(p) => p,
                };

                Analyzer::build_member(
                    iter.1,
                    properties,
                    &title_must_exist.to_string(),
                    &self.map_of_struct,
                    &mut current_member,
                );
            } else if iter.0 == "anyOf" {
                self.map_of_enum.insert(title_must_exist.to_string(), true);

                let array: &Vec<serde_json::Value> = match iter.1.as_array() {
                    None => continue,
                    Some(a) => a,
                };
                for sub_item in array {
                    //TODO: need more security&border check

                    let requreid = match sub_item.get("required") {
                        None => continue,
                        Some(r) => r,
                    };

                    let name = match requreid[0].as_str() {
                        None => continue,
                        Some(n) => n,
                    };

                    let required = match sub_item.get("properties") {
                        None => continue,
                        Some(p) => match p.get(name) {
                            None => continue,
                            Some(nm) => nm,
                        },
                    };

                    let properties = match required.get("properties") {
                        None => required,
                        Some(pp) => pp,
                    };

                    let target_required = match required.as_object() {
                        None => continue,
                        Some(target) => match target.get("required") {
                            None => &serde_json::Value::Null,
                            Some(m) => m,
                        },
                    };

                    if name != "null" {
                        Analyzer::build_member(
                            target_required,
                            properties,
                            &name.to_string(),
                            &self.map_of_struct,
                            &mut current_member,
                        );
                    }
                }
            }
        }
        return true;
    }

    pub fn show_message_type(&self, name: &str, members: &Vec<Member>) {
        println!("{} {{", name.blue().bold());
        for vcm in members {
            let st = match self.map_of_struct.get_key_value(vcm.member_def.as_str()) {
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

    //load jsonschema file, translate from json string to func:params...
    pub fn try_load_json_schema<P: AsRef<std::path::Path>>(&mut self, dir: P) -> bool {
        let all_json_file = match std::fs::read_dir(dir) {
            Err(_e) => return false,
            Ok(f) => f,
        };

        for file in all_json_file {
            if !self.analyze_schema(file.unwrap().path().display().to_string()) {
                return false;
            }
        }

        return true;
    }
}

pub fn from_json_schema(file_path: &String, schema_path: &str) -> Analyzer {
    let mut analyzer = Analyzer::default();
    let seg = match file_path.rfind('/') {
        None => return analyzer,
        Some(idx) => idx,
    };
    let (parent_path, _) = file_path.split_at(seg);
    if cfg!(debug_assertions) {
        println!(
            "Auto loading json schema from {}/{}",
            parent_path, schema_path
        );
    }
    analyzer.try_load_json_schema(std::path::Path::new(parent_path).join(schema_path));
    analyzer
}

pub fn load_data_from_file(path: &str) -> Result<Vec<u8>, String> {
    let mut file = match File::open(path) {
        Err(e) => return Err(format!("failed to open file , error: {}", e).to_string()),
        Ok(f) => f,
    };
    let mut data = Vec::<u8>::new();
    let _size = match file.read_to_end(&mut data) {
        Err(e) => return Err(format!("failed to read wasm , error: {}", e).to_string()),
        Ok(sz) => sz,
    };
    Ok(data)
}
