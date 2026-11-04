//! A versioned JSON request into the same pipeline as the CLI, without argument parsing.
use crate::{mutation_config, request::MutationRequest};
use errspec::mutation::{Report, options::Options};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    io::{self, Read},
    path::PathBuf,
    process::ExitCode,
};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct Input {
    protocol: u8,
    options: Value,
    config_file: Option<PathBuf>,
}

pub(crate) fn entry() -> ExitCode {
    let acquire = || -> Result<MutationRequest, String> {
        let mut bytes = vec![];
        io::stdin()
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map_err(|e| e.to_string())?;
        if bytes.len() > 1024 * 1024 {
            return Err("API request exceeds 1 MiB".into());
        }
        let mut input: Input = serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
        if input.protocol != 1 {
            return Err("Unsupported mutation API protocol".into());
        }
        let object = input
            .options
            .as_object_mut()
            .ok_or("options must be an object")?;
        if let Some(schema) = object.remove("$schema")
            && !schema.is_string()
        {
            return Err("$schema must be a string".into());
        }
        let mut overrides = BTreeSet::new();
        let snake = |value: &str| {
            let mut output = String::new();
            for c in value.chars() {
                if c.is_uppercase() {
                    output.push('_');
                    output.extend(c.to_lowercase());
                } else {
                    output.push(c);
                }
            }
            output
        };
        for key in object.keys().filter(|key| key.as_str() != "policy") {
            overrides.insert(snake(key));
        }
        if let Some(policy) = object.get("policy").and_then(Value::as_object) {
            for key in policy.keys() {
                overrides.insert(snake(key));
            }
        }
        let options: Options = serde_json::from_value(input.options).map_err(|e| e.to_string())?;
        Ok(MutationRequest {
            options,
            config_file: input.config_file,
            overrides,
        })
    };
    let report = match acquire() {
        Ok(request) => mutation_config::run(request),
        Err(error) => {
            let mut report = Report::new(false);
            report.error("EC_MUTATION_CONFIG", error);
            report.finish();
            report
        }
    };
    let code = report.exit_code();
    println!("{}", json!({"protocol":1,"exitCode":code,"report":report}));
    ExitCode::from(code)
}
