//! Private sequential JSON Lines generator protocol for mutation generation.
use super::source_protocol;
use crate::mutation_source;
use serde::Deserialize;
use serde_json::{Value, json};
use std::io::{self, BufRead, Read, Write};
use std::process::ExitCode;
const LIMIT: u64 = 64 * 1024 * 1024;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    version: u8,
    id: u64,
    request: Value,
}
fn serve() -> Result<(), String> {
    let stdin = io::stdin();
    let mut reader = stdin.lock();
    let stdout = io::stdout();
    let mut writer = stdout.lock();
    let mut last_id = 0;
    loop {
        let mut line = Vec::new();
        let count = (&mut reader)
            .take(LIMIT + 1)
            .read_until(b'\n', &mut line)
            .map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(());
        }
        if count as u64 > LIMIT || !line.ends_with(b"\n") {
            return Err("Oversized or truncated generator frame".into());
        }
        let raw: Value =
            serde_json::from_slice(&line).map_err(|e| format!("Invalid generator JSON: {e}"))?;
        let request: Request =
            serde_json::from_value(raw).map_err(|e| format!("Invalid generator request: {e}"))?;
        if request.version != 1 || request.id <= last_id {
            return Err("Invalid generator version or request ID".into());
        }
        last_id = request.id;
        // No parser, semantic, graph or arena references survive this request.
        let response = match source_protocol::decode(request.request)
            .and_then(mutation_source::generate)
        {
            Ok(result) => {
                json!({"version":1,"id":last_id,"result":result})
            }
            Err(message) => {
                if message.starts_with("EC_MUTATION_PROTOCOL:") {
                    return Err(message);
                }
                json!({"version":1,"id":last_id,"error":{"kind":if message.starts_with("EC_MUTATION_INPUT:"){"input"}else{"syntax"},"message":message}})
            }
        };
        let output = serde_json::to_vec(&response).map_err(|e| e.to_string())?;
        if output.len() as u64 + 1 > LIMIT {
            return Err("Generator response exceeds 64 MiB".into());
        }
        writer
            .write_all(&output)
            .and_then(|_| writer.write_all(b"\n"))
            .and_then(|_| writer.flush())
            .map_err(|e| e.to_string())?;
    }
}
pub(crate) fn entry() -> ExitCode {
    let result = std::thread::Builder::new()
        .stack_size(32 * 1024 * 1024)
        .spawn(serve)
        .map_err(|e| e.to_string())
        .and_then(|t| t.join().map_err(|_| "Mutation parser panicked".into()))
        .and_then(|r| r);
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}
