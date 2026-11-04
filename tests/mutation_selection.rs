use errspec::mutation::selection::{Lines, hunks, validate};
use serde_json::json;
use sha2::{Digest, Sha256};

#[test]
fn changed_lines_cover_insertions_deletions_and_single_line_hunks() {
    let ranges=hunks("diff --git a/quoted b/quoted\n@@ -0,0 +1,2 @@\n+new\n+lines\n@@ -4 +6,0 @@\n-old\n@@ -9 +10 @@\n-old\n+new\n").unwrap();
    assert_eq!(
        ranges,
        vec![
            Lines { start: 1, end: 2 },
            Lines { start: 6, end: 7 },
            Lines { start: 10, end: 10 }
        ]
    );
    assert!(hunks("@@ malformed @@").is_err());
    assert!(hunks("@@ -1 +invalid @@").is_err());
}

#[test]
fn invalid_scope_cycles_and_utf8_ranges_fail_before_selection() {
    let key = "a".repeat(64);
    let candidate=serde_json::from_value(json!({"key":key,"file":"src/a.ts","line":1,"byteOffset":0,"end":4,"mutator":"StringLiteral","original":"'é'","replacement":"''","placement":"expression","ignored":null,"regex":null,"guardOffset":null})).unwrap();
    let mut facts:errspec::mutation::syntax::Metadata=serde_json::from_value(json!({"version":2,"sourceHash":format!("{:x}",Sha256::digest("'é'".as_bytes())),"scopes":{"module":{"hash":"a".repeat(64),"effectHash":"a".repeat(64),"start":0,"end":4,"line":1,"endLine":1,"parent":null,"name":null,"references":[],"pure":false,"function":false,"unknownCalls":false}},"owners":{key:"module"},"nodes":[]})).unwrap();
    assert!(validate(&facts, "'é'", std::slice::from_ref(&candidate)).is_ok());
    facts.scopes.get_mut("module").unwrap().parent = Some("module".into());
    assert!(validate(&facts, "'é'", &[]).unwrap_err().contains("Cyclic"));
    assert!(
        validate(&facts, "'é'", std::slice::from_ref(&candidate))
            .unwrap_err()
            .contains("Cyclic")
    );
    facts.scopes.get_mut("module").unwrap().parent = None;
    facts.scopes.get_mut("module").unwrap().end = 2;
    assert!(validate(&facts, "'é'", std::slice::from_ref(&candidate)).is_err());
}
