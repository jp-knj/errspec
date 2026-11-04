use errspec::{
    model::FailurePath,
    reach::{self, LineHits},
};
use std::{collections::BTreeMap, path::PathBuf};

fn path(line: usize, tags: &[&str]) -> FailurePath {
    FailurePath {
        file: "src/submit.ts".into(),
        line,
        byte_offset: line as u32 * 10,
        tags: tags.iter().map(|t| t.to_string()).collect(),
        reached: None,
    }
}

fn resolved(hits: LineHits) -> BTreeMap<PathBuf, BTreeMap<usize, u64>> {
    hits.into_iter()
        .map(|(file, lines)| (PathBuf::from("/p").join(file), lines))
        .collect()
}

fn resolve(file: &str) -> Option<PathBuf> {
    Some(PathBuf::from("/p").join(file))
}

// [正常系]

#[test]
fn lcov_line_hits_are_read_per_source_file() {
    let hits = reach::parse_lcov(
        "TN:\nSF:src/submit.ts\nFN:1,submit\nDA:1,1\nDA:2,0\nLF:2\nLH:1\nend_of_record\nSF:src/other.ts\nDA:5,3\nend_of_record\n",
    )
    .unwrap();
    assert_eq!(hits["src/submit.ts"], BTreeMap::from([(1, 1), (2, 0)]));
    assert_eq!(hits["src/other.ts"], BTreeMap::from([(5, 3)]));
}

#[test]
fn reached_and_unreached_paths_are_marked() {
    let hits =
        resolved(reach::parse_lcov("SF:src/submit.ts\nDA:3,2\nDA:7,0\nend_of_record\n").unwrap());
    let mut paths = vec![path(3, &["blocked"]), path(7, &["blocked"])];
    let findings = reach::judge("submit", &mut paths, &hits, resolve);
    assert_eq!(paths[0].reached, Some(true));
    assert_eq!(paths[1].reached, Some(false));
    assert_eq!(findings.len(), 1);
    assert_eq!(findings[0].code, "EC019");
    assert_eq!(findings[0].contract_id.as_deref(), Some("submit"));
    assert_eq!(findings[0].file.as_deref(), Some("src/submit.ts"));
    assert_eq!(findings[0].byte_offset, Some(70));
    assert!(findings[0].message.contains("blocked (line 7)"));
}

#[test]
fn a_path_with_several_tags_names_all_of_them() {
    let hits = resolved(reach::parse_lcov("SF:src/submit.ts\nDA:4,0\nend_of_record\n").unwrap());
    let mut paths = vec![path(4, &["blocked", "invalid"])];
    let findings = reach::judge("submit", &mut paths, &hits, resolve);
    assert!(
        findings[0].message.contains("blocked | invalid"),
        "{}",
        findings[0].message
    );
}

#[test]
fn all_paths_reached_report_nothing() {
    let hits =
        resolved(reach::parse_lcov("SF:src/submit.ts\nDA:1,1\nDA:2,5\nend_of_record\n").unwrap());
    let mut paths = vec![path(1, &["a"]), path(2, &["b"])];
    assert!(reach::judge("submit", &mut paths, &hits, resolve).is_empty());
    assert!(paths.iter().all(|p| p.reached == Some(true)));
}

// [異常系]

#[test]
fn malformed_or_orphaned_line_records_are_rejected() {
    for text in [
        "SF:a.ts\nDA:x,1\n",
        "SF:a.ts\nDA:1\n",
        "SF:a.ts\nDA:1,-1\n",
        "DA:1,1\n",
        "SF:a.ts\nDA:1,1\nend_of_record\nDA:2,1\n",
    ] {
        assert!(reach::parse_lcov(text).is_err(), "{text:?}");
    }
}

#[test]
fn a_file_missing_from_coverage_was_never_loaded() {
    let hits = resolved(reach::parse_lcov("SF:src/other.ts\nDA:1,1\nend_of_record\n").unwrap());
    let mut paths = vec![path(1, &["a"]), path(9, &["b"])];
    let findings = reach::judge("submit", &mut paths, &hits, resolve);
    assert_eq!(findings.len(), 2);
    assert!(paths.iter().all(|p| p.reached == Some(false)));
}

#[test]
fn an_unresolvable_path_counts_as_unreached() {
    let hits = resolved(reach::parse_lcov("SF:src/submit.ts\nDA:1,1\nend_of_record\n").unwrap());
    let mut paths = vec![path(1, &["a"])];
    let findings = reach::judge("submit", &mut paths, &hits, |_| None);
    assert_eq!(findings.len(), 1);
}

// [エッジケース]

#[test]
fn repeated_source_records_accumulate_hits() {
    let hits = reach::parse_lcov(
        "SF:a.ts\nDA:1,0\nend_of_record\nSF:a.ts\nDA:1,2\nDA:2,0\nend_of_record\n",
    )
    .unwrap();
    assert_eq!(hits["a.ts"], BTreeMap::from([(1, 2), (2, 0)]));
}

#[test]
fn a_line_without_coverage_data_is_not_judged_unreached() {
    // Coverage can omit lines it does not consider executable; that is not evidence of a gap.
    let hits = resolved(reach::parse_lcov("SF:src/submit.ts\nDA:1,1\nend_of_record\n").unwrap());
    let mut paths = vec![path(2, &["a"])];
    assert!(reach::judge("submit", &mut paths, &hits, resolve).is_empty());
    assert_eq!(paths[0].reached, Some(true));
}

#[test]
fn an_empty_report_has_no_files() {
    assert!(reach::parse_lcov("").unwrap().is_empty());
    let hits = reach::parse_lcov("SF:a.ts\nend_of_record\n").unwrap();
    assert!(hits["a.ts"].is_empty());
}
