use serde_json::Value;
use std::path::Path;

/// Replace only this fixture's absolute directory prefix, including its canonical spelling.
/// Relative paths, sibling directories, identities, positions and array order stay intact.
pub fn project(value: &Value, root: &Path) -> Value {
    fn replace(value: &mut Value, prefix: &str) {
        match value {
            Value::String(text) => *text = text.replace(prefix, "<project>/"),
            Value::Array(values) => values.iter_mut().for_each(|v| replace(v, prefix)),
            Value::Object(values) => values.values_mut().for_each(|v| replace(v, prefix)),
            _ => (),
        }
    }

    let mut value = value.clone();
    // Canonical first: on macOS /var is an alias for /private/var.
    for root in [root.canonicalize().unwrap(), root.to_path_buf()] {
        replace(&mut value, &format!("{}/", root.to_string_lossy()));
    }
    value
}
