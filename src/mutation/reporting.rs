//! Pure report rendering over acquired source snapshots. No filesystem or process access.
use super::{Mutant, Status, TestResult, syntax::Scope};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
    str::FromStr,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Reporter {
    Text,
    Json,
    Html,
    Agent,
    Simplify,
    Annotations,
}
impl FromStr for Reporter {
    type Err = String;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        serde_json::from_value(json!(value)).map_err(|_| format!("Unknown reporter: {value}"))
    }
}
pub fn validate_reporters(reporters: &[Reporter]) -> Result<(), String> {
    let stdout = reporters
        .iter()
        .filter(|r| !matches!(r, Reporter::Json | Reporter::Html))
        .count();
    if stdout > 1 {
        return Err("Choose one stdout reporter: text, agent, simplify, or annotations".into());
    }
    let mut seen = BTreeSet::new();
    if reporters.iter().any(|r| !seen.insert(format!("{r:?}"))) {
        return Err("Duplicate reporter".into());
    }
    Ok(())
}
#[derive(Debug, Default)]
pub struct Presentation {
    pub root: PathBuf,
    pub reporters: Vec<Reporter>,
    pub directory: Option<PathBuf>,
}

/// The presentation consumes only owned public report facts; judgments stay in the run pipeline.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Input {
    pub kind: String,
    pub schema_version: u8,
    pub run_complete: bool,
    pub ok: bool,
    pub tests_executed: bool,
    pub mutants: Vec<Mutant>,
    pub baseline: Vec<TestResult>,
    pub summary: Value,
    pub gate: Value,
    pub diagnostics: Vec<Value>,
    pub policy: Option<Value>,
    pub selection: Option<Value>,
}
#[derive(Default)]
pub struct Sources {
    pub files: BTreeMap<String, Source>,
    pub warnings: Vec<Value>,
}
pub struct Source {
    pub text: String,
    pub scopes: BTreeMap<String, Scope>,
}

pub fn gap(mutant: &Mutant) -> Option<&'static str> {
    match mutant.status {
        Status::NoCoverage => Some("notReached"),
        Status::Survived if mutant.infected_by.as_ref().is_some_and(Vec::is_empty) => {
            Some("notInfected")
        }
        Status::Survived => Some("notDetected"),
        _ => None,
    }
}
fn fix(gap: &str) -> &'static str {
    match gap {
        "notReached" => {
            "Add a test that executes this code. Check whether a spawning test executes it outside the instrumented worker."
        }
        "notInfected" => {
            "Add an input for which the original and replacement differ, such as a boundary or a value outside the checked range."
        }
        _ => {
            "Assert the observable result changed by the mutation: return value, error, state, or calls. If equivalent for every relevant input, accept it with a reason."
        }
    }
}
fn reason(mutant: &Mutant) -> String {
    let mut reasons = vec![];
    if let Some(gap) = gap(mutant) {
        reasons.push(
            match gap {
                "notReached" => "not reached",
                "notInfected" => "not infected",
                _ => "not detected",
            }
            .to_owned(),
        );
    }
    if mutant.critical {
        reasons.push("critical region: requires detection or reasoned acceptance".into());
    }
    if let Some(accepted) = &mutant.accepted {
        reasons.push(format!("accepted: {accepted}"));
    }
    reasons.extend(
        mutant
            .reason
            .iter()
            .chain(mutant.candidate.ignored.iter())
            .cloned(),
    );
    if reasons.is_empty() {
        reasons.extend(mutant.attempts.last().map(|a| a.reason.clone()));
    }
    reasons.join("; ")
}
fn position(text: &str, offset: usize) -> Result<Value, String> {
    let before = text.get(..offset).ok_or("Invalid report source offset")?;
    let column = before
        .rsplit('\n')
        .next()
        .unwrap_or("")
        .encode_utf16()
        .count()
        + 1;
    Ok(json!({"line":before.bytes().filter(|b|*b==b'\n').count()+1,"column":column}))
}
fn location(mutant: &Mutant, sources: &Sources) -> Result<Value, String> {
    let source = &sources
        .files
        .get(&mutant.candidate.location.file)
        .ok_or("Missing report source")?
        .text;
    let start = mutant.candidate.location.byte_offset as usize;
    let end = mutant.candidate.end as usize;
    if source.get(start..end) != Some(mutant.candidate.original.as_str()) {
        return Err(format!(
            "Source changed at mutant {}: rerun mutation or supply its original source snapshot",
            mutant.candidate.key
        ));
    }
    let start = position(source, start)?;
    if start["line"] != mutant.candidate.location.line {
        return Err("Report source line disagrees with mutant".into());
    }
    Ok(json!({"start":start,"end":position(source,end)?}))
}
fn warnings(input: &Input, sources: &Sources) -> Vec<Value> {
    let mut out = sources.warnings.clone();
    if let Some(selection) = &input.selection {
        out.extend(
            selection["warnings"]
                .as_array()
                .into_iter()
                .flatten()
                .cloned(),
        );
    }
    out.extend(input.diagnostics.iter().cloned());
    out
}
fn threshold(input: &Input, key: &str, default: u64) -> u64 {
    // The upstream MTE schema requires integer thresholds. Preserve fractional policy below.
    input
        .policy
        .as_ref()
        .and_then(|p| p["options"][key].as_f64())
        .map_or(default, |v| v.ceil() as u64)
}
pub fn mutation_testing_json(
    input: &Input,
    sources: &Sources,
    root: &str,
) -> Result<Value, String> {
    let mut files: BTreeMap<String, Value> = BTreeMap::new();
    for mutant in &input.mutants {
        let file = &mutant.candidate.location.file;
        let source = &sources.files.get(file).ok_or("Missing report source")?.text;
        let extension = file.rsplit('.').next().unwrap_or("");
        let entry = files.entry(file.clone()).or_insert_with(|| json!({"language":if ["ts","tsx","mts","cts"].contains(&extension){"typescript"}else{"javascript"},"source":source,"mutants":[]}));
        entry["mutants"].as_array_mut().unwrap().push(json!({
            "id":mutant.candidate.key,"mutatorName":mutant.candidate.mutator,
            "replacement":mutant.candidate.replacement,"location":location(mutant,sources)?,
            "status":mutant.status,"coveredBy":mutant.covered_by,"killedBy":mutant.killed_by,
            "statusReason":reason(mutant),"gap":gap(mutant),"reused":mutant.reused,
        }));
    }
    let mut tests: BTreeMap<String, Value> = BTreeMap::new();
    for test in &input.baseline {
        tests
            .entry(test.file.clone())
            .or_insert_with(|| json!({"tests":[]}))["tests"]
            .as_array_mut()
            .unwrap()
            .push(json!({"id":test.id,"name":test.name}));
    }
    Ok(
        json!({"schemaVersion":"2","framework":{"name":"errspec","version":env!("CARGO_PKG_VERSION")},"projectRoot":root,
            "thresholds":{"high":threshold(input,"thresholdHigh",80),"low":threshold(input,"thresholdLow",60)},
            "files":files,"testFiles":tests,"warnings":warnings(input,sources),
            "errspec":{"runComplete":input.run_complete,"ok":input.ok,"summary":input.summary,"gate":input.gate,"policy":input.policy,"selection":input.selection}
        }),
    )
}
fn removal(mutant: &Mutant) -> Option<&'static str> {
    let c = &mutant.candidate;
    match c.mutator.as_str() {
        "CallExpression" if c.replacement.trim() == ";" || c.replacement.is_empty() => {
            Some("statement")
        }
        "BlockStatement" if c.replacement.replace(' ', "") == "{}" => Some("block"),
        "ConditionalExpression" if ["true", "false"].contains(&c.replacement.as_str()) => {
            Some("condition")
        }
        "OptionalChaining" => Some("optionalChain"),
        "MethodExpression"
            if !c.replacement.is_empty()
                && c.original.len() > c.replacement.len()
                && c.original.starts_with(&c.replacement) =>
        {
            Some("methodCall")
        }
        _ => None,
    }
}
fn scope(mutant: &Mutant, sources: &Sources) -> Result<Value, String> {
    let source = sources
        .files
        .get(&mutant.candidate.location.file)
        .ok_or("Missing report source")?;
    let start = mutant.candidate.location.byte_offset as usize;
    let end = mutant.candidate.end as usize;
    let enclosing = source
        .scopes
        .iter()
        .filter(|(_, s)| s.function && s.start <= start && end <= s.end)
        .min_by_key(|(_, s)| s.end - s.start)
        .or_else(|| source.scopes.get_key_value("module"));
    let (id, s) = enclosing.ok_or("Missing report scope")?;
    Ok(
        json!({"id":id,"name":s.name,"startLine":s.line,"endLine":s.end_line,"source":source.text.get(s.start..s.end).ok_or("Invalid report scope")?}),
    )
}
pub fn agent(input: &Input, sources: &Sources) -> Result<Value, String> {
    let mut gaps = vec![];
    for mutant in &input.mutants {
        let Some(gap) = gap(mutant) else {
            continue;
        };
        let location = location(mutant, sources)?;
        let scope = scope(mutant, sources)?;
        let name = scope["name"].as_str().unwrap_or("").to_lowercase();
        let mut tests = mutant.covered_by.clone();
        if !name.is_empty() {
            tests.sort_by_key(|id| !id.to_lowercase().contains(&name));
        }
        tests.truncate(10);
        gaps.push(json!({"key":mutant.candidate.key,"gap":gap,"file":mutant.candidate.location.file,
            "line":location["start"]["line"],"column":location["start"]["column"],"location":location,
            "mutator":mutant.candidate.mutator,"original":mutant.candidate.original,"replacement":mutant.candidate.replacement,
            "scope":scope,"coveredBy":tests,"coveredByCount":mutant.covered_by.len(),"fix":fix(gap),
            "critical":mutant.critical,"simplify":if mutant.status==Status::Survived {removal(mutant)}else{None},"reason":reason(mutant)}));
    }
    Ok(
        json!({"kind":"errspec/mutation-agent","schemaVersion":1,"runComplete":input.run_complete,
        "summary":input.summary,"gate":input.gate,"gaps":gaps,"warnings":warnings(input,sources),"selection":input.selection}),
    )
}
fn simplify(input: &Input, sources: &Sources) -> Result<String, String> {
    let mut lines = vec![
        "May be unnecessary: covered code whose removal is not observed by a test.".to_owned(),
        "Review each candidate, then remove it or add a test that needs it.".into(),
    ];
    let mut count = 0;
    for mutant in &input.mutants {
        if mutant.status != Status::Survived || mutant.covered_by.is_empty() {
            continue;
        }
        let Some(kind) = removal(mutant) else {
            continue;
        };
        let point = location(mutant, sources)?;
        let scope = scope(mutant, sources)?;
        lines.push(format!(
            "{}:{}:{} {} [{}] {}: {} -> {}{}",
            mutant.candidate.location.file,
            point["start"]["line"],
            point["start"]["column"],
            scope["id"].as_str().unwrap_or("module"),
            mutant.candidate.key,
            kind,
            mutant.candidate.original.replace('\n', " "),
            mutant.candidate.replacement.replace('\n', " "),
            if gap(mutant) == Some("notInfected") {
                " (not infected)"
            } else {
                ""
            }
        ));
        count += 1;
    }
    if count == 0 {
        lines.push("No candidates.".into());
    }
    Ok(lines.join("\n"))
}
fn escape_command(value: &str, property: bool) -> String {
    let value = value
        .replace('%', "%25")
        .replace('\r', "%0D")
        .replace('\n', "%0A");
    if property {
        value.replace(':', "%3A").replace(',', "%2C")
    } else {
        value
    }
}
pub fn annotations(input: &Input, sources: &Sources) -> Result<String, String> {
    let mut lines = vec![];
    for mutant in &input.mutants {
        let Some(gap) = gap(mutant) else {
            continue;
        };
        if input.selection.is_none() && !mutant.critical {
            continue;
        }
        let point = location(mutant, sources)?;
        let message = format!(
            "{}: {} -> {}. {} [{}]",
            mutant.candidate.mutator,
            mutant.candidate.original,
            mutant.candidate.replacement,
            fix(gap),
            mutant.candidate.key
        );
        lines.push(format!(
            "::warning file={},line={},col={},endLine={},endColumn={},title={}::{}",
            escape_command(&mutant.candidate.location.file, true),
            point["start"]["line"],
            point["start"]["column"],
            point["end"]["line"],
            point["end"]["column"],
            gap,
            escape_command(&message, false)
        ));
    }
    for warning in input
        .selection
        .as_ref()
        .and_then(|s| s["warnings"].as_array())
        .into_iter()
        .flatten()
    {
        lines.push(format!(
            "::warning file={},title=Uncollected changed test::{}",
            escape_command(warning["file"].as_str().unwrap_or(""), true),
            escape_command(warning["message"].as_str().unwrap_or(""), false)
        ));
    }
    Ok(lines.join("\n"))
}
pub fn format(
    input: &Input,
    reporter: Reporter,
    sources: &Sources,
    root: &str,
) -> Result<String, String> {
    if input.kind != "errspec/mutation" || input.schema_version != 1 {
        return Err("Unsupported mutation report".into());
    }
    match reporter {
        Reporter::Json => {
            serde_json::to_string_pretty(&mutation_testing_json(input, sources, root)?)
                .map_err(|e| e.to_string())
        }
        Reporter::Html => Ok(html(&mutation_testing_json(input, sources, root)?)),
        Reporter::Agent => {
            serde_json::to_string_pretty(&agent(input, sources)?).map_err(|e| e.to_string())
        }
        Reporter::Simplify => simplify(input, sources),
        Reporter::Annotations => annotations(input, sources),
        Reporter::Text => {
            let mut lines = vec![
                format!(
                    "mutate: {} (complete: {}, tests executed: {})",
                    if input.ok { "pass" } else { "fail" },
                    input.run_complete,
                    input.tests_executed
                ),
                format!("summary: {}", input.summary),
            ];
            for mutant in &input.mutants {
                lines.push(format!(
                    "{}:{} {} {:?}: {} -> {} ({})",
                    mutant.candidate.location.file,
                    mutant.candidate.location.line,
                    mutant.candidate.mutator,
                    mutant.status,
                    mutant.candidate.original.replace('\n', " "),
                    mutant.candidate.replacement.replace('\n', " "),
                    reason(mutant)
                ));
            }
            for warning in warnings(input, sources) {
                lines.push(format!(
                    "warning: {}",
                    warning["message"].as_str().unwrap_or("")
                ));
            }
            Ok(lines.join("\n"))
        }
    }
}
pub fn html(report: &Value) -> String {
    // Sources and test names are inert JSON data; use textContent throughout the viewer.
    let data = serde_json::to_string(report)
        .unwrap()
        .replace('<', "\\u003c")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    HTML.replace("__REPORT_DATA__", &data)
}
const HTML: &str = include_str!("report-viewer.html");
