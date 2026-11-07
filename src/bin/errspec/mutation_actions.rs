//! Filesystem entry points for explicit mutation workflow actions.
use errspec::mutation::policy::Acceptance;
use serde_json::json;
use std::{collections::BTreeMap, fs, io::Write, path::Path};

pub(crate) use crate::command_output::Output as ActionOutput;

pub(crate) fn accept(root: &Path, file: &Path, keys: &[String], reason: &str) -> ActionOutput {
    let result = (|| -> Result<_, String> {
        if reason.trim().is_empty() {
            return Err("Approval requires a nonempty reason".into());
        }
        if keys.is_empty()
            || keys
                .iter()
                .any(|key| key.len() != 64 || !key.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err("Approval requires complete mutant keys".into());
        }
        let root = root.canonicalize().map_err(|e| e.to_string())?;
        let file = root.join(file);
        let mut accepted: BTreeMap<String, Acceptance> = match fs::read(&file) {
            Ok(bytes) => {
                serde_json::from_slice(&bytes).map_err(|e| format!("{}: {e}", file.display()))?
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => BTreeMap::new(),
            Err(error) => return Err(error.to_string()),
        };
        for key in keys {
            accepted.insert(
                key.clone(),
                Acceptance {
                    reason: reason.trim().into(),
                    r#where: None,
                },
            );
        }
        let parent = file
            .parent()
            .ok_or("Approval file needs a parent directory")?;
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
        let mut bytes = serde_json::to_vec_pretty(&accepted).map_err(|e| e.to_string())?;
        bytes.push(b'\n');
        temporary
            .write_all(&bytes)
            .and_then(|_| temporary.as_file().sync_all())
            .map_err(|e| e.to_string())?;
        temporary.persist(&file).map_err(|e| e.error.to_string())?;
        Ok((file, keys.len()))
    })();
    match result {
        Ok((file, count)) => ActionOutput {
            json: json!({"kind":"errspec/mutation-accept","schemaVersion":1,"ok":true,"file":file,"accepted":keys,"reason":reason.trim()}),
            text: format!("Accepted {count} mutants: {}", file.display()),
            code: 0,
        },
        Err(message) => ActionOutput {
            json: json!({"kind":"errspec/mutation-accept","schemaVersion":1,"ok":false,"diagnostics":[{"code":"EC_MUTATION_CONFIG","message":message}]}),
            text: format!("EC_MUTATION_CONFIG {message}"),
            code: 2,
        },
    }
}
