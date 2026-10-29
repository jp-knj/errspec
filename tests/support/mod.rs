use errspec::{config::ValidatedConfig, contract, evidence::ResolvedPaths, report::ReportData};

pub fn register<'a>(config: &'a ValidatedConfig, tags: &[&str]) -> ReportData<'a> {
    let mut data = ReportData {
        analysis_complete: true,
        ..Default::default()
    };
    for contract in config.contracts() {
        let files = contract
            .cases()
            .iter()
            .map(|case| case.test().file.clone())
            .collect();
        let (registration, findings) = contract::compare(
            contract,
            tags.iter().map(|tag| (*tag).to_owned()).collect(),
            &files,
        );
        data.registrations.push(registration);
        data.diagnostics.extend(findings);
    }
    data
}

// Deliberately synthetic file identities: pure judgments must not touch the filesystem.
pub fn paths(files: &[&str]) -> ResolvedPaths {
    files
        .iter()
        .map(|file| {
            (
                (*file).to_owned(),
                std::path::PathBuf::from("virtual").join(file),
            )
        })
        .collect()
}
