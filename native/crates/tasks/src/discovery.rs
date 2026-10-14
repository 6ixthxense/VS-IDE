//! Task detection (port of `discoverPackageScriptTasks`).

use std::fs;
use std::path::Path;

use serde_json::Value;

use vsm_core::paths::locale_compare;
use crate::types::{TaskDefinition, TaskSource};

/// `npm run <script>` tasks for every entry in `package.json` `scripts`, sorted by name.
/// Missing or invalid `package.json` yields an empty list.
pub fn discover_package_script_tasks(root_path: &Path) -> Vec<TaskDefinition> {
    let Ok(text) = fs::read_to_string(root_path.join("package.json")) else {
        return Vec::new();
    };
    let Ok(Value::Object(package)) = serde_json::from_str::<Value>(&text) else {
        return Vec::new();
    };
    let Some(Value::Object(scripts)) = package.get("scripts") else {
        return Vec::new();
    };

    let npm_command = if cfg!(windows) { "npm.cmd" } else { "npm" };
    let mut entries: Vec<(&String, &Value)> = scripts.iter().collect();
    entries.sort_by(|(left, _), (right, _)| locale_compare(left, right));

    entries
        .into_iter()
        .map(|(script_name, script_command)| TaskDefinition {
            id: format!("npm:{script_name}"),
            label: script_name.clone(),
            command: npm_command.to_string(),
            args: vec!["run".to_string(), script_name.clone()],
            source: TaskSource::PackageScript,
            cwd: Some(root_path.to_path_buf()),
            detail: Some(match script_command {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            }),
            shell: Some(cfg!(windows)),
        })
        .collect()
}
