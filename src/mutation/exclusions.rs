//! Explicit comment exclusions over owned lexical facts, independent of parsers.
#[derive(Debug)]
pub struct Comment {
    pub start: usize,
    pub end: usize,
    pub text: String,
}
enum Scope {
    Line(usize),
    Region(usize),
}
struct Directive {
    disabled: bool,
    families: Vec<String>,
    reason: String,
    scope: Scope,
}
#[derive(Default)]
pub struct Disabled {
    directives: Vec<Directive>,
}
fn line(source: &str, offset: usize) -> usize {
    // JavaScript recognizes CR, LF, CRLF and the Unicode line separators.
    let mut previous_cr = false;
    let mut number = 1;
    for character in source[..offset].chars() {
        if character == '\r'
            || matches!(character, '\u{2028}' | '\u{2029}')
            || (character == '\n' && !previous_cr)
        {
            number += 1;
        }
        previous_cr = character == '\r';
    }
    number
}
impl Disabled {
    pub fn new(source: &str, mut comments: Vec<Comment>) -> Self {
        comments.sort_by_key(|comment| comment.start);
        let directives = comments
            .into_iter()
            .filter_map(|comment| {
                if comment.start > comment.end
                    || comment.end > source.len()
                    || !source.is_char_boundary(comment.start)
                    || !source.is_char_boundary(comment.end)
                {
                    return None;
                }
                let body = comment.text.trim().trim_start_matches('*').trim();
                let (command, reason) = body.split_once(':').unwrap_or((body, ""));
                let mut words = command.split_whitespace();
                let first = words.next()?;
                let (disabled, scope) = if first == "Stryker" {
                    let disabled = match words.next()? {
                        "disable" => true,
                        "restore" => false,
                        _ => return None,
                    };
                    let next_line = words.clone().next() == Some("next-line");
                    if next_line {
                        words.next();
                    }
                    words.clone().next()?;
                    (
                        disabled,
                        if disabled && next_line {
                            Scope::Line(line(source, comment.end) + 1)
                        } else {
                            Scope::Region(comment.end)
                        },
                    )
                } else {
                    let verb = first
                        .strip_prefix("errspec-")
                        .or_else(|| first.strip_prefix("mutator-"))?;
                    match verb {
                        "disable-next-line" => (true, Scope::Line(line(source, comment.end) + 1)),
                        "disable-line" => (true, Scope::Line(line(source, comment.start))),
                        "disable" => (true, Scope::Region(comment.end)),
                        "enable" => (false, Scope::Region(comment.end)),
                        _ => return None,
                    }
                };
                let mut families = words
                    .flat_map(|word| word.split(','))
                    .filter(|name| !name.is_empty())
                    .map(str::to_owned)
                    .collect::<Vec<_>>();
                if families
                    .iter()
                    .any(|name| !name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_'))
                {
                    return None;
                }
                if families.iter().any(|name| name.eq_ignore_ascii_case("all")) {
                    families.clear();
                }
                let reason = reason.split_whitespace().collect::<Vec<_>>().join(" ");
                Some(Directive {
                    disabled,
                    families,
                    scope,
                    reason: if reason.is_empty() {
                        "disabled".into()
                    } else {
                        format!("disabled: {reason}")
                    },
                })
            })
            .collect();
        Self { directives }
    }
    pub fn reason(&self, source: &str, family: &str, offset: usize) -> Option<String> {
        if self.directives.is_empty() {
            return None;
        }
        let number = line(source, offset);
        let matches = |directive: &&Directive| {
            directive.families.is_empty() || directive.families.iter().any(|name| name == family)
        };
        let mut region = None;
        for directive in self.directives.iter().filter(matches) {
            match directive.scope {
                Scope::Line(target) if target == number => return Some(directive.reason.clone()),
                Scope::Region(start) if start <= offset => {
                    region = directive.disabled.then_some(directive)
                }
                _ => (),
            }
        }
        region.map(|directive| directive.reason.clone())
    }
}
