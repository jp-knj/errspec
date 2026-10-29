use errspec::model::Finding;
use std::{fs, path::Path};

pub(crate) fn read_json<T: serde::de::DeserializeOwned>(
    path: &Path,
    code: &str,
) -> Result<T, Finding> {
    let bytes =
        fs::read(path).map_err(|e| Finding::new(code, format!("{}: {e}", path.display())))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| Finding::new(code, format!("{}: {e}", path.display())))
}
