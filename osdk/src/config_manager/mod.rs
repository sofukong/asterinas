// SPDX-License-Identifier: MPL-2.0

//! This module is responsible for parsing configuration files and combining them with command-line parameters
//! to obtain the final configuration, it will also try searching system to fill valid values for specific
//! arguments if the arguments is missing, e.g., the path of QEMU. The final configuration is stored in `BuildConfig`,
//! `RunConfig` and `TestConfig`. These `*Config` are used for `build`, `run` and `test` subcommand.

pub mod boot;
pub mod manifest;
pub mod qemu;

use std::path::PathBuf;
use std::{fs, process};

use indexmap::{IndexMap, IndexSet};
use which::which;

use self::boot::BootLoader;
use self::manifest::{OsdkManifest, TomlManifest};
use crate::cli::{BuildArgs, CargoArgs, OsdkArgs, RunArgs, TestArgs};
use crate::error::Errno;
use crate::utils::get_cargo_metadata;
use crate::{error_msg, warn_msg};

/// Configurations for build subcommand
#[derive(Debug)]
pub struct BuildConfig {
    pub manifest: OsdkManifest,
    pub cargo_args: CargoArgs,
}

impl BuildConfig {
    pub fn parse(args: &BuildArgs) -> Self {
        let cargo_args = split_features(&args.cargo_args);
        let mut manifest = load_osdk_manifest(&cargo_args);
        apply_cli_args(&mut manifest, &args.osdk_args);
        try_fill_system_configs(&mut manifest);
        Self {
            manifest,
            cargo_args,
        }
    }
}

/// Configurations for run subcommand
#[derive(Debug)]
pub struct RunConfig {
    pub manifest: OsdkManifest,
    pub cargo_args: CargoArgs,
}

impl RunConfig {
    pub fn parse(args: &RunArgs) -> Self {
        let cargo_args = split_features(&args.cargo_args);
        let mut manifest = load_osdk_manifest(&cargo_args);
        apply_cli_args(&mut manifest, &args.osdk_args);
        try_fill_system_configs(&mut manifest);
        Self {
            manifest,
            cargo_args,
        }
    }
}

/// Configurations for test subcommand
#[derive(Debug)]
pub struct TestConfig {
    pub manifest: OsdkManifest,
    pub cargo_args: CargoArgs,
    pub test_name: Option<String>,
}

impl TestConfig {
    pub fn parse(args: &TestArgs) -> Self {
        let cargo_args = split_features(&args.cargo_args);
        let mut manifest = load_osdk_manifest(&cargo_args);
        apply_cli_args(&mut manifest, &args.osdk_args);
        try_fill_system_configs(&mut manifest);
        Self {
            manifest,
            cargo_args,
            test_name: args.test_name.clone(),
        }
    }
}

fn load_osdk_manifest(cargo_args: &CargoArgs) -> OsdkManifest {
    let manifest_path = {
        let feature_strings = get_feature_strings(cargo_args);
        let cargo_metadata = get_cargo_metadata(None::<&str>, Some(&feature_strings));
        let workspace_root = cargo_metadata
            .get("workspace_root")
            .unwrap()
            .as_str()
            .unwrap();
        PathBuf::from(workspace_root).join("OSDK.toml")
    };

    let Ok(contents) = fs::read_to_string(&manifest_path) else {
        error_msg!(
            "Cannot read file {}",
            manifest_path.to_string_lossy().to_string()
        );
        process::exit(Errno::GetMetadata as _);
    };

    let toml_manifest: TomlManifest = toml::from_str(&contents).unwrap();
    OsdkManifest::from_toml_manifest(toml_manifest, &cargo_args.features)
}

/// Split `features` in `cargo_args` to ensure each string contains exactly one feature.
/// This method will spilt features seperated by comma in one string as multiple strings.
fn split_features(cargo_args: &CargoArgs) -> CargoArgs {
    let mut features = Vec::new();

    for feature in cargo_args.features.iter() {
        for feature in feature.split(',') {
            if !feature.is_empty() {
                features.push(feature.to_string());
            }
        }
    }

    CargoArgs {
        release: cargo_args.release,
        features,
    }
}

pub fn get_feature_strings(cargo_args: &CargoArgs) -> Vec<String> {
    cargo_args
        .features
        .iter()
        .map(|feature| format!("--features={}", feature))
        .collect()
}

pub fn try_fill_system_configs(manifest: &mut OsdkManifest) {
    if manifest.qemu.path.is_none() {
        if let Ok(path) = which("qemu-system-x86_64") {
            trace!("system qemu path: {:?}", path);
            manifest.qemu.path = Some(path);
        } else {
            warn_msg!("Cannot find qemu-system-x86_64 in your system. ")
        }
    }

    if manifest.boot.grub_mkrescue.is_none() && manifest.boot.loader == BootLoader::Grub {
        if let Ok(path) = which("grub-mkrescue") {
            trace!("system grub-mkrescue path: {:?}", path);
            manifest.boot.grub_mkrescue = Some(path);
        } else {
            warn_msg!("Cannot find grub-mkrescue in your system.")
        }
    }
}

pub fn apply_cli_args(manifest: &mut OsdkManifest, args: &OsdkArgs) {
    let mut init_args = split_kcmd_args(&mut manifest.kcmd_args);
    apply_kv_array(&mut manifest.kcmd_args, &args.kcmd_args, "=", &[]);
    init_args.append(&mut args.init_args.clone());

    manifest.kcmd_args.push("--".to_string());
    for init_arg in init_args {
        for seperated_arg in init_arg.split(' ') {
            manifest.kcmd_args.push(seperated_arg.to_string());
        }
    }

    apply_option(&mut manifest.initramfs, &args.initramfs);
    apply_option(&mut manifest.boot.ovmf, &args.boot_ovmf);
    apply_option(&mut manifest.boot.grub_mkrescue, &args.boot_grub_mkrescue);
    apply_item(&mut manifest.boot.loader, &args.boot_loader);
    apply_item(&mut manifest.boot.protocol, &args.boot_protocol);
    apply_option(&mut manifest.qemu.path, &args.qemu_path);
    apply_item(&mut manifest.qemu.machine, &args.qemu_machine);

    // check qemu_args
    for arg in manifest.qemu.args.iter() {
        qemu::check_qemu_arg(arg);
    }
    for arg in args.qemu_args.iter() {
        qemu::check_qemu_arg(arg);
    }

    apply_kv_array(
        &mut manifest.qemu.args,
        &args.qemu_args,
        " ",
        qemu::MULTI_VALUE_KEYS,
    );
}

fn apply_item<'a, T: From<&'a str> + Clone>(item: &mut T, arg: &Option<T>) {
    let Some(arg) = arg.clone() else {
        return;
    };

    *item = arg;
}

fn apply_option<'a, T: From<&'a str> + Clone>(item: &mut Option<T>, arg: &Option<T>) {
    let Some(arg) = arg.clone() else {
        return;
    };

    *item = Some(arg);
}

pub fn apply_kv_array(
    array: &mut Vec<String>,
    args: &Vec<String>,
    seperator: &str,
    multi_value_keys: &[&str],
) {
    let multi_value_keys = {
        let mut inferred_keys = infer_multi_value_keys(array, seperator);
        for key in multi_value_keys {
            inferred_keys.insert(key.to_string());
        }
        inferred_keys
    };

    debug!("multi value keys: {:?}", multi_value_keys);

    // We use IndexMap to keep key orders
    let mut key_strings = IndexMap::new();
    let mut multi_value_key_strings: IndexMap<String, Vec<String>> = IndexMap::new();
    for item in array.drain(..) {
        // Each key-value string has two patterns:
        // 1. Seperated by separator: key value / key=value
        if let Some(key) = get_key(&item, seperator) {
            if multi_value_keys.contains(&key) {
                if let Some(v) = multi_value_key_strings.get_mut(&key) {
                    v.push(item);
                } else {
                    let v = vec![item];
                    multi_value_key_strings.insert(key, v);
                }
                continue;
            }

            key_strings.insert(key, item);
            continue;
        }
        // 2. Only key, no value
        key_strings.insert(item.clone(), item);
    }

    for arg in args {
        if let Some(key) = get_key(arg, seperator) {
            if multi_value_keys.contains(&key) {
                if let Some(v) = multi_value_key_strings.get_mut(&key) {
                    v.push(arg.to_owned());
                } else {
                    let v = vec![arg.to_owned()];
                    multi_value_key_strings.insert(key, v);
                }
                continue;
            }

            key_strings.insert(key, arg.to_owned());
            continue;
        }

        key_strings.insert(arg.to_owned(), arg.to_owned());
    }

    *array = key_strings.into_iter().map(|(_, value)| value).collect();

    for (_, mut values) in multi_value_key_strings {
        array.append(&mut values);
    }
}

fn infer_multi_value_keys(array: &Vec<String>, seperator: &str) -> IndexSet<String> {
    let mut multi_val_keys = IndexSet::new();

    let mut occured_keys = IndexSet::new();
    for item in array {
        let Some(key) = get_key(item, seperator) else {
            continue;
        };

        if occured_keys.contains(&key) {
            multi_val_keys.insert(key);
        } else {
            occured_keys.insert(key);
        }
    }

    multi_val_keys
}

pub fn get_key(item: &str, seperator: &str) -> Option<String> {
    let split = item.split(seperator).collect::<Vec<_>>();
    let len = split.len();
    if len > 2 || len == 0 {
        error_msg!("`{}` is an invalid argument.", item);
        process::exit(Errno::ParseMetadata as _);
    }

    if len == 1 {
        return None;
    }

    let key = split.first().unwrap();

    Some(key.to_string())
}

fn split_kcmd_args(kcmd_args: &mut Vec<String>) -> Vec<String> {
    let seperator = "--";
    let index = kcmd_args.iter().position(|item| item.as_str() == seperator);
    let Some(index) = index else {
        return Vec::new();
    };
    let mut init_args = kcmd_args.split_off(index);
    init_args.remove(0);
    init_args
}

#[test]
fn split_kcmd_args_test() {
    let mut kcmd_args = ["init=/bin/sh", "--", "sh", "-l"]
        .iter()
        .map(ToString::to_string)
        .collect();
    let init_args = split_kcmd_args(&mut kcmd_args);
    let expected_kcmd_args: Vec<_> = ["init=/bin/sh"].iter().map(ToString::to_string).collect();
    assert_eq!(kcmd_args, expected_kcmd_args);
    let expecetd_init_args: Vec<_> = ["sh", "-l"].iter().map(ToString::to_string).collect();
    assert_eq!(init_args, expecetd_init_args);

    let mut kcmd_args = ["init=/bin/sh", "--"]
        .iter()
        .map(ToString::to_string)
        .collect();
    let init_args = split_kcmd_args(&mut kcmd_args);
    let expected_kcmd_args: Vec<_> = ["init=/bin/sh"].iter().map(ToString::to_string).collect();
    assert_eq!(kcmd_args, expected_kcmd_args);
    let expecetd_init_args: Vec<String> = Vec::new();
    assert_eq!(init_args, expecetd_init_args);

    let mut kcmd_args = ["init=/bin/sh", "shell=/bin/sh"]
        .iter()
        .map(ToString::to_string)
        .collect();
    let init_args = split_kcmd_args(&mut kcmd_args);
    let expected_kcmd_args: Vec<_> = ["init=/bin/sh", "shell=/bin/sh"]
        .iter()
        .map(ToString::to_string)
        .collect();
    assert_eq!(kcmd_args, expected_kcmd_args);
    let expecetd_init_args: Vec<String> = Vec::new();
    assert_eq!(init_args, expecetd_init_args);
}
