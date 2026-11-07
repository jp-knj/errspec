//! Acquire installation targets, plan embedded skill copies, and apply explicit writes.
use crate::command_output::Output;
use serde_json::json;
use std::{
    collections::BTreeMap,
    fs,
    io::Write,
    path::{Path, PathBuf},
};
struct Template {
    name: &'static str,
    file: &'static str,
    bytes: &'static [u8],
}
const FILES: &[Template] = &[
    Template {
        name: "errspec",
        file: "errspec/SKILL.md",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/cli/skills/errspec/SKILL.md"
        )),
    },
    Template {
        name: "errspec",
        file: "errspec/references/configuration.md",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/cli/skills/errspec/references/configuration.md"
        )),
    },
    Template {
        name: "errspec",
        file: "errspec/references/runners.md",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/cli/skills/errspec/references/runners.md"
        )),
    },
    Template {
        name: "errspec-playwright",
        file: "errspec-playwright/SKILL.md",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/cli/skills/errspec-playwright/SKILL.md"
        )),
    },
    Template {
        name: "errspec-test-loop",
        file: "errspec-test-loop/SKILL.md",
        bytes: include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/packages/cli/skills/errspec-test-loop/SKILL.md"
        )),
    },
];
struct Planned {
    template: &'static Template,
    before: Option<Vec<u8>>,
    changed: bool,
}
fn read_target(path: &Path) -> Result<Option<Vec<u8>>, String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => {
            if metadata.file_type().is_symlink() || !metadata.is_file() {
                return Err(format!(
                    "Skill target is not a regular file: {}",
                    path.display()
                ));
            }
            if metadata.len() > 1024 * 1024 {
                return Err(format!(
                    "Existing skill file exceeds 1 MiB: {}",
                    path.display()
                ));
            }
            fs::read(path).map(Some).map_err(|e| e.to_string())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}
fn check_parents(root: &Path, file: &Path) -> Result<(), String> {
    let mut current = Some(
        file.parent()
            .ok_or("Skill target needs a parent directory")?,
    );
    while let Some(path) = current {
        if !path.starts_with(root) {
            break;
        }
        match fs::symlink_metadata(path) {
            Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
                return Err(format!(
                    "Skill directory must be a real directory: {}",
                    path.display()
                ));
            }
            Ok(_) => (),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error.to_string()),
        }
        current = path.parent();
    }
    Ok(())
}
fn plan(existing: &BTreeMap<&str, Option<Vec<u8>>>, force: bool) -> Result<Vec<Planned>, String> {
    FILES
        .iter()
        .map(|template| {
            let before = existing[template.file].clone();
            let changed = before.as_deref() != Some(template.bytes);
            if changed && before.is_some() && !force {
                return Err(format!(
                    "{} differs from the shipped skill; use --force to replace it",
                    template.file
                ));
            }
            Ok(Planned {
                template,
                before,
                changed,
            })
        })
        .collect()
}
fn atomic_write(
    path: &Path,
    bytes: &[u8],
    replace: bool,
    permissions: Option<&fs::Permissions>,
) -> Result<(), String> {
    let parent = path
        .parent()
        .ok_or("Skill target needs a parent directory")?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent).map_err(|e| e.to_string())?;
    temporary.write_all(bytes).map_err(|e| e.to_string())?;
    if let Some(permissions) = permissions {
        temporary
            .as_file()
            .set_permissions(permissions.clone())
            .map_err(|e| e.to_string())?;
    }
    temporary.as_file().sync_all().map_err(|e| e.to_string())?;
    if replace {
        temporary.persist(path).map_err(|e| e.error.to_string())?;
    } else {
        temporary
            .persist_noclobber(path)
            .map_err(|e| e.error.to_string())?;
    }
    Ok(())
}
pub(crate) fn run(directory: &Path, force: bool) -> Output {
    let result = || -> Result<_, String> {
        if directory.to_str().is_none() {
            return Err("Skill directory must be UTF-8".into());
        }
        if directory.as_os_str().is_empty() {
            return Err("Skill directory must not be empty".into());
        }
        let cwd = std::env::current_dir()
            .and_then(|path| path.canonicalize())
            .map_err(|e| e.to_string())?;
        if directory.is_relative() {
            let mut ancestor = cwd.clone();
            for component in directory.components() {
                ancestor.push(component);
                if fs::symlink_metadata(&ancestor)
                    .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return Err(format!(
                        "Relative skill directory must not traverse a symlink: {}",
                        ancestor.display()
                    ));
                }
            }
        }
        let root = cwd.join(directory);
        let mut existing = BTreeMap::new();
        for template in FILES {
            let target = root.join(template.file);
            check_parents(&root, &target)?;
            existing.insert(template.file, read_target(&target)?);
        }
        let plan = plan(&existing, force)?;
        let mut applied: Vec<(&Planned, Option<fs::Permissions>)> = vec![];
        let apply = || -> Result<(), String> {
            for change in plan.iter().filter(|entry| entry.changed) {
                let target = root.join(change.template.file);
                check_parents(&root, &target)?;
                if read_target(&target)? != change.before {
                    return Err(format!(
                        "Skill target changed during installation: {}",
                        target.display()
                    ));
                }
                fs::create_dir_all(target.parent().unwrap()).map_err(|e| e.to_string())?;
                let permissions = change
                    .before
                    .as_ref()
                    .map(|_| fs::metadata(&target).map(|metadata| metadata.permissions()))
                    .transpose()
                    .map_err(|e| e.to_string())?;
                atomic_write(
                    &target,
                    change.template.bytes,
                    change.before.is_some(),
                    permissions.as_ref(),
                )?;
                applied.push((change, permissions));
            }
            Ok(())
        };
        let mut apply = apply;
        if let Err(error) = apply() {
            let mut failures = vec![];
            for (change, permissions) in applied.into_iter().rev() {
                let target = root.join(change.template.file);
                if check_parents(&root, &target).is_err()
                    || read_target(&target).ok().flatten().as_deref() != Some(change.template.bytes)
                {
                    failures.push(format!("{} changed during rollback", target.display()));
                    continue;
                }
                let restored = if let Some(before) = &change.before {
                    atomic_write(&target, before, true, permissions.as_ref())
                } else {
                    fs::remove_file(&target).map_err(|e| e.to_string())
                };
                if let Err(error) = restored {
                    failures.push(error);
                }
            }
            return Err(if failures.is_empty() {
                error
            } else {
                format!("{error}; rollback: {}", failures.join("; "))
            });
        }
        let mut skills: BTreeMap<&str, bool> = BTreeMap::new();
        for change in &plan {
            *skills.entry(change.template.name).or_default() |= change.changed;
        }
        Ok(skills.into_iter().map(|(name,changed)|json!({"name":name,"path":directory.join(name),"status":if changed{"written"}else{"unchanged"}})).collect::<Vec<_>>())
    };
    match result() {
        Ok(skills) => Output {
            json: json!({"kind":"errspec/skills-init","schemaVersion":1,"ok":true,"skills":skills}),
            text: skills
                .iter()
                .map(|entry| {
                    format!(
                        "{} {}",
                        entry["status"].as_str().unwrap(),
                        PathBuf::from(entry["path"].as_str().unwrap()).display()
                    )
                })
                .collect::<Vec<_>>()
                .join("\n"),
            code: 0,
        },
        Err(message) => Output {
            json: json!({"kind":"errspec/skills-init","schemaVersion":1,"ok":false,"diagnostics":[{"code":"EC_SKILL_INIT","message":message}]}),
            text: format!("EC_SKILL_INIT {message}"),
            code: 2,
        },
    }
}
