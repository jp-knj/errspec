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

/// Registration checks happen before the runner can create or remove files.
pub(crate) fn existing_tests(
    root: &Path,
    contract: &errspec::config::ValidatedContract,
) -> std::collections::BTreeSet<String> {
    contract
        .cases()
        .iter()
        .filter(|case| root.join(&case.test().file).is_file())
        .map(|case| case.test().file.clone())
        .collect()
}

pub(crate) fn resolve_paths<'a>(
    root: &Path,
    files: impl IntoIterator<Item = &'a str>,
) -> errspec::evidence::ResolvedPaths {
    files
        .into_iter()
        .filter_map(|file| {
            root.join(file)
                .canonicalize()
                .ok()
                .map(|path| (file.to_owned(), path))
        })
        .collect()
}
