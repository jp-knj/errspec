use errspec::{observation::RunContext, observation_io};
use std::fs;

#[test]
fn observation_size_limit_accepts_the_boundary_and_rejects_one_byte_more() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("one.json");
    let run = RunContext::new("fixture-run");
    for size in [1_048_575, 1_048_576, 1_048_577] {
        let mut bytes = include_bytes!("fixtures/observation-v1.json").to_vec();
        bytes.resize(size, b' ');
        fs::write(&path, bytes).unwrap();
        let result = observation_io::read(dir.path(), &run);
        if size <= 1_048_576 {
            assert_eq!(result.unwrap().records().len(), 1);
        } else {
            let error = result.unwrap_err();
            assert_eq!(error.code, "EC_EVIDENCE");
            assert_eq!(&*error.message, "Observation size limit exceeded");
        }
    }
}

#[test]
fn first_sorted_record_is_validated_before_later_files_are_parsed() {
    let dir = tempfile::tempdir().unwrap();
    // Creation order differs from filename order. Both files fail for different reasons.
    fs::write(dir.path().join("b.json"), "{").unwrap();
    fs::write(
        dir.path().join("a.json"),
        include_bytes!("fixtures/observation-v1.json"),
    )
    .unwrap();
    let error = observation_io::read(dir.path(), &RunContext::new("other-run")).unwrap_err();
    assert_eq!(
        &*error.message,
        "Observation protocol or current run id does not match"
    );
    let error = observation_io::read(dir.path(), &RunContext::new("fixture-run")).unwrap_err();
    assert!(error.message.starts_with("Invalid observation JSON:"));
}
