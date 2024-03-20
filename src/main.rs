// Get input file and output filename from cmd line! (positional args)
// Open and read line-by-line
// Parse each line into a struct
// filtering out anything that is not AntithesisAssert{}
// and inserts into a map<id, Vec<struct>
//
// Now with each key in map
// - do we have an item in the vec with hit==true && cond==true => passed:= true;  hit==false;
// - determine if each assertion was passed or failed
// Output each item with pass/fail indication (and other info) to JSON output file
//

use std::env;
use std::fs;
use serde::{ Deserialize, Serialize };
use serde_json::{ Value };
use anyhow::{ Result, bail };
use std::collections::HashMap;
use std::io::Write;

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct AntithesisSdk {
    language: String, 
    version: String 
}

#[derive(Deserialize, Debug)]
#[allow(dead_code)]
struct AntithesisSetup {
    status: String,
    details: Value,
}

#[derive(Deserialize, Serialize, Debug)]
struct Location {
    begin_column: i32,
    begin_line: i32,
    class: String,
    file: String,
    function: String,
}

#[derive(Deserialize, Debug)]
struct AntithesisAssert {
    assert_type: AssertType,
    condition: bool,
    display_type: String,
    hit: bool,
    must_hit: bool,
    id: String,
    message: String,
    location: Location,
    details: Value,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum SDKInput {
    AntithesisSdk(AntithesisSdk),
    AntithesisAssert(AntithesisAssert),
    AntithesisSetup(AntithesisSetup),

    #[allow(dead_code)]
    SendEvent{event_name: String, details: Value }
}

#[derive(Serialize, Debug)]
struct EvaluatedAssertion {
    display_type: String,
    id: String,
    message: String,
    location: Location,
    example_details: Option<Value>,
    counter_details: Option<Value>,
    passed: bool,
}

#[derive(Deserialize, Debug)]
#[serde(rename_all = "snake_case")]
enum AssertType {
    Always,
    Sometimes,
    Reachability,
}

impl EvaluatedAssertion {
    fn new(assert_list: Vec<AntithesisAssert>) -> Self {

        let mut catalog_entry = None;
        let mut condition_true_entry = None;
        let mut condition_false_entry = None;

        for entry in assert_list {
            let hit = entry.hit;
            if hit {
                let condition = entry.condition;
                if condition {
                    condition_true_entry = Some(entry);
                } else {
                    condition_false_entry = Some(entry);
                }
            } else {
                catalog_entry = Some(entry);
            }
        }

        // TODO Handle requests that do not even have a catalog_entry
        let input_entry = catalog_entry.unwrap();

        let passed: bool;
        let mut example_details = None;
        let mut counter_details = None;

        match input_entry.assert_type {
            AssertType::Always => {
                let must_hit = input_entry.must_hit;
                if must_hit {
                    passed = condition_true_entry.is_some() &&  condition_false_entry.is_none();
                } else {
                    passed = condition_false_entry.is_none();
                }
                example_details = condition_true_entry.map(|x| x.details);
                counter_details = condition_false_entry.map(|x| x.details);
            },
            AssertType::Sometimes => {
                passed = condition_true_entry.is_some();
                example_details = condition_true_entry.map(|x| x.details);
                // TODO Do we really want to show details for a sometimes that failed?
                counter_details = condition_false_entry.map(|x| x.details);
            },
            AssertType::Reachability => {
                let hit = condition_true_entry.is_some() || condition_false_entry.is_some();
                let must_hit = input_entry.must_hit;
                if must_hit {
                    passed = hit;
                    example_details =  condition_true_entry.or(condition_false_entry).map(|x| x.details);
                } else {
                    passed = !hit;
                    counter_details =  condition_true_entry.or(condition_false_entry).map(|x| x.details);
                }
            },
        }

        let evaled = Self {
            display_type: input_entry.display_type,
            id: input_entry.id,
            message: input_entry.message,
            location: input_entry.location,
            passed,
            example_details,
            counter_details,
        };
        evaled 
    }
}


fn group_asserts(inputs: Vec<SDKInput>) -> HashMap<String, Vec<AntithesisAssert>> {
    let mut result  = HashMap::new();
    for input in inputs {
        match input {
            SDKInput::AntithesisAssert(x) => {
               let entry = result.entry(x.id.clone()).or_insert(Vec::new()); 
               entry.push(x);
            },
            _ => {
                eprintln!("IGNORE: {:?}", input);
            },
        }
    }
    result
}

fn parse_lines(lines: Vec<&str>) -> Result<Vec<SDKInput>> {
    let mut result = Vec::new();

    for line in lines {
        if line.len() < 1 { continue; }
        let parsed: SDKInput = match serde_json::from_str(line) {
            Ok(x) => x,
            Err(_e) => {
                // println!("{}", line);
                // println!("PARSING: {:?}", e);
                let temp: Value = serde_json::from_str(line)?; 
                // should be Object(Map<String, Value>)
                // in this case the Map has just one entry (top-level name used by SendEvent())
                match temp {
                    Value::Object(user_data) => {
                       let mut result = None;
                       for (event_name, details) in user_data {
                            result = Some(SDKInput::SendEvent{
                                event_name,
                                details,
                            });
                            break;
                       } 
                        match result {
                            Some(x) => x,
                            None => bail!("no details found here")
                        }
                    },
                    _ => bail!("it broke - not an Object() unable to parse JSON")
                }
            }
        };
        result.push(parsed);
    }
    Ok(result)
}

fn main() -> Result<()>{
    let args: Vec<String> = env::args().collect();
    if args.len() < 3 {
        panic!("Usage: {} input_file output_file ...", args[0]);
    }
    let input_file = &args[1];
    let output_file = &args[2];
    
    let contents = fs::read_to_string(input_file)
        .expect("Should have been able to read the file");
    
    let lines = contents.split("\n");
    let parsed = parse_lines(lines.collect())?;
    let grouped_assertions = group_asserts(parsed);

    // After into_values() the map is no longer useable
    let evaled_assertions: Vec<_> = grouped_assertions.into_values().map(|one_vec| EvaluatedAssertion::new(one_vec)).collect();
    // dbg!(&evaled_assertions);
    
    let mut file = fs::File::create(output_file)?;

    for evaled_assertion in evaled_assertions {
        let s = serde_json::to_string(&evaled_assertion)?;
        file.write_all(s.as_bytes())?;
        file.write_all(b"\n")?;
    }

    Ok(())
}
