// SPDX-License-Identifier: MPL-2.0

//! The common utils for crate unit test

use assert_cmd::Command;
use std::ffi::OsStr;
use std::fs::{self, create_dir_all};
use std::path::{Path, PathBuf};
use std::process::Output;

pub fn cargo_osdk<T: AsRef<OsStr>, I: IntoIterator<Item = T>>(args: I) -> Command {
    let mut command = Command::cargo_bin("cargo-osdk").unwrap();
    command.arg("osdk");
    command.args(args);
    command
}

pub fn assert_success(output: &Output) {
    assert!(output.status.success());
}

pub fn assert_stdout_contains_msg(output: &Output, msg: &str) {
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains(msg))
}

pub fn create_workspace(workspace_name: &str, members: &[&str]) {
    let mut table = toml::Table::new();
    let workspace_table = {
        let mut table = toml::Table::new();
        table.insert("resolver".to_string(), toml::Value::String("2".to_string()));

        let members = members
            .iter()
            .map(|member| toml::Value::String(member.to_string()))
            .collect();
        table.insert("members".to_string(), toml::Value::Array(members));
        table
    };

    table.insert("workspace".to_string(), toml::Value::Table(workspace_table));

    create_dir_all(workspace_name).unwrap();
    let manefest_path = PathBuf::from(workspace_name).join("Cargo.toml");

    let content = table.to_string();
    fs::write(manefest_path, content).unwrap();

    let osdk_manifest_path = PathBuf::from(workspace_name).join("OSDK.toml");
    fs::write(osdk_manifest_path, "").unwrap();
}

pub fn add_member_to_workspace(workspace: impl AsRef<Path>, new_member: &str) {
    let path = PathBuf::from(workspace.as_ref()).join("Cargo.toml");

    let mut workspace_manifest: toml::Table = {
        let content = fs::read_to_string(&path).unwrap();
        toml::from_str(&content).unwrap()
    };

    let members = workspace_manifest
        .get_mut("workspace")
        .unwrap()
        .get_mut("members")
        .unwrap();
    if let toml::Value::Array(members) = members {
        members.push(toml::Value::String(new_member.to_string()));
    }

    let new_content = workspace_manifest.to_string();
    fs::write(&path, new_content).unwrap();
}
