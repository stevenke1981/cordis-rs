//! Local capability discovery. This crate probes and reports; it never installs tools.

use cordis_contracts::{CAPABILITY_INDEX_SCHEMA, now_rfc3339};
use cordis_store::{CordisStore, StoreError};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::env;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CapabilityError {
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    #[error("invalid capability request: {0}")]
    Validation(String),
    #[error("required tool is unavailable: {0}")]
    Unavailable(String),
}

pub type CapabilityResult<T> = Result<T, CapabilityError>;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum CapabilityScope {
    #[default]
    Project,
    Global,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ToolProbeSpec {
    #[serde(default)]
    pub command: Option<String>,
    #[serde(default)]
    pub path: Option<String>,
    #[serde(default = "default_version_args")]
    pub version_args: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub scope: CapabilityScope,
}

fn default_version_args() -> Vec<String> {
    vec!["--version".to_owned()]
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CapabilityEntry {
    pub name: String,
    pub scope: CapabilityScope,
    pub path: String,
    pub version: String,
    pub verify_args: Vec<String>,
    pub capabilities: Vec<String>,
    pub available: bool,
    pub reason: String,
    pub source: String,
    pub last_verified_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RegisterCapability {
    pub name: String,
    pub path: String,
    #[serde(default)]
    pub version: String,
    #[serde(default)]
    pub verify_args: Vec<String>,
    #[serde(default)]
    pub capabilities: Vec<String>,
    #[serde(default)]
    pub scope: CapabilityScope,
}

#[derive(Clone)]
pub struct CapabilityIndex {
    store: CordisStore,
}

impl CapabilityIndex {
    pub fn new(store: CordisStore) -> Self {
        Self { store }
    }

    pub fn register(&self, request: RegisterCapability) -> CapabilityResult<CapabilityEntry> {
        validate_name(&request.name)?;
        if request.path.trim().is_empty() {
            return Err(CapabilityError::Validation(
                "path must be non-empty".to_owned(),
            ));
        }
        let path = PathBuf::from(request.path.trim());
        let available = path.is_file();
        let entry = CapabilityEntry {
            name: request.name.trim().to_owned(),
            scope: request.scope,
            path: path.display().to_string(),
            version: request.version.trim().to_owned(),
            verify_args: request.verify_args,
            capabilities: dedupe(request.capabilities),
            available,
            reason: if available {
                "ok"
            } else {
                "declared_path_missing"
            }
            .to_owned(),
            source: "declared".to_owned(),
            last_verified_at: now_rfc3339(),
        };
        self.store.save_capability(&entry.name, &entry)?;
        self.store.audit("capability_registered", None, &entry)?;
        Ok(entry)
    }

    pub fn detect(
        &self,
        candidates: BTreeMap<String, ToolProbeSpec>,
    ) -> CapabilityResult<BTreeMap<String, CapabilityEntry>> {
        let mut result = BTreeMap::new();
        for (name, spec) in candidates {
            validate_name(&name)?;
            let existing: Option<CapabilityEntry> = self.store.load_capability(&name)?;
            let observed = probe(&name, &spec);
            let entry = CapabilityEntry {
                name: name.clone(),
                scope: existing.as_ref().map_or(spec.scope, |item| item.scope),
                path: observed.path,
                version: if observed.version.is_empty() {
                    existing
                        .as_ref()
                        .map_or_else(String::new, |item| item.version.clone())
                } else {
                    observed.version
                },
                verify_args: spec.version_args,
                capabilities: if spec.capabilities.is_empty() {
                    existing
                        .as_ref()
                        .map_or_else(Vec::new, |item| item.capabilities.clone())
                } else {
                    dedupe(spec.capabilities)
                },
                available: observed.available,
                reason: observed.reason,
                source: if observed.available {
                    "detected"
                } else {
                    "probe"
                }
                .to_owned(),
                last_verified_at: now_rfc3339(),
            };
            self.store.save_capability(&name, &entry)?;
            result.insert(name, entry);
        }
        self.store.audit("capabilities_detected", None, &result)?;
        Ok(result)
    }

    pub fn get(&self, name: &str) -> CapabilityResult<CapabilityEntry> {
        self.store
            .load_capability(name)?
            .ok_or_else(|| CapabilityError::Validation(format!("unknown tool: {name}")))
    }

    pub fn require(&self, name: &str) -> CapabilityResult<CapabilityEntry> {
        let entry = self.get(name)?;
        if !entry.available {
            return Err(CapabilityError::Unavailable(format!(
                "{} ({})",
                entry.name, entry.reason
            )));
        }
        Ok(entry)
    }

    pub fn is_available(&self, name: &str) -> CapabilityResult<bool> {
        Ok(self.get(name)?.available)
    }

    pub fn status(&self) -> CapabilityResult<Value> {
        let entries: Vec<CapabilityEntry> = self.store.list_capabilities()?;
        let tools: BTreeMap<_, _> = entries
            .into_iter()
            .map(|entry| (entry.name.clone(), entry))
            .collect();
        Ok(json!({
            "schema": CAPABILITY_INDEX_SCHEMA,
            "updated_at": now_rfc3339(),
            "tools": tools,
        }))
    }
}

struct ProbeResult {
    available: bool,
    path: String,
    version: String,
    reason: String,
}

fn probe(name: &str, spec: &ToolProbeSpec) -> ProbeResult {
    let candidate = spec
        .path
        .as_deref()
        .or(spec.command.as_deref())
        .unwrap_or(name)
        .trim();
    let Some(path) = resolve_executable(candidate) else {
        return ProbeResult {
            available: false,
            path: String::new(),
            version: String::new(),
            reason: "not_found_on_path".to_owned(),
        };
    };
    let mut child = match Command::new(&path)
        .args(&spec.version_args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(error) => {
            return ProbeResult {
                available: false,
                path: path.display().to_string(),
                version: String::new(),
                reason: format!("spawn_failed:{error}"),
            };
        }
    };
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                return ProbeResult {
                    available: false,
                    path: path.display().to_string(),
                    version: String::new(),
                    reason: "probe_timeout".to_owned(),
                };
            }
            Err(error) => {
                return ProbeResult {
                    available: false,
                    path: path.display().to_string(),
                    version: String::new(),
                    reason: format!("probe_failed:{error}"),
                };
            }
        }
    }
    match child.wait_with_output() {
        Ok(output) => {
            let text = if output.stdout.is_empty() {
                output.stderr
            } else {
                output.stdout
            };
            let version = String::from_utf8_lossy(&text)
                .lines()
                .next()
                .unwrap_or_default()
                .trim()
                .chars()
                .take(200)
                .collect();
            ProbeResult {
                available: output.status.success(),
                path: path.display().to_string(),
                version,
                reason: if output.status.success() {
                    "ok".to_owned()
                } else {
                    format!("probe_exit:{}", output.status.code().unwrap_or(-1))
                },
            }
        }
        Err(error) => ProbeResult {
            available: false,
            path: path.display().to_string(),
            version: String::new(),
            reason: format!("output_failed:{error}"),
        },
    }
}

fn resolve_executable(candidate: &str) -> Option<PathBuf> {
    let path = Path::new(candidate);
    if path.is_absolute() || candidate.contains(std::path::MAIN_SEPARATOR) {
        return path.is_file().then(|| path.to_path_buf());
    }
    let path_var = env::var_os("PATH")?;
    let extensions = executable_extensions();
    for directory in env::split_paths(&path_var) {
        for extension in &extensions {
            let mut name = OsString::from(candidate);
            name.push(extension);
            let full = directory.join(name);
            if full.is_file() {
                return Some(full);
            }
        }
    }
    None
}

fn executable_extensions() -> Vec<OsString> {
    #[cfg(windows)]
    {
        let path_ext =
            env::var_os("PATHEXT").unwrap_or_else(|| OsString::from(".EXE;.CMD;.BAT;.COM"));
        let mut values = vec![OsString::new()];
        values.extend(path_ext.to_string_lossy().split(';').map(OsString::from));
        values
    }
    #[cfg(not(windows))]
    {
        vec![OsString::new()]
    }
}

fn validate_name(name: &str) -> CapabilityResult<()> {
    if name.trim().is_empty() {
        Err(CapabilityError::Validation(
            "tool name must be non-empty".to_owned(),
        ))
    } else {
        Ok(())
    }
}

fn dedupe(values: Vec<String>) -> Vec<String> {
    let mut values: Vec<_> = values
        .into_iter()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .collect();
    values.sort();
    values.dedup();
    values
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn index() -> CapabilityIndex {
        let path = tempdir().unwrap().keep().join("capability.db");
        CapabilityIndex::new(CordisStore::open(path).unwrap())
    }

    #[test]
    fn declared_path_is_registered_without_installation() {
        let index = index();
        let executable = std::env::current_exe().unwrap();
        let entry = index
            .register(RegisterCapability {
                name: "test-runner".to_owned(),
                path: executable.display().to_string(),
                version: "test".to_owned(),
                verify_args: vec!["--list".to_owned()],
                capabilities: vec!["tests".to_owned()],
                scope: CapabilityScope::Project,
            })
            .unwrap();
        assert!(entry.available);
        assert!(index.require("test-runner").is_ok());
    }

    #[test]
    fn missing_required_tool_fails_closed() {
        let index = index();
        index
            .register(RegisterCapability {
                name: "missing".to_owned(),
                path: "/definitely/not/here".to_owned(),
                version: String::new(),
                verify_args: vec![],
                capabilities: vec![],
                scope: CapabilityScope::Project,
            })
            .unwrap();
        assert!(index.require("missing").is_err());
    }
}
