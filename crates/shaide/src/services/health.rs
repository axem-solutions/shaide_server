use std::env::consts::ARCH;

use serde::{Deserialize, Serialize};
use sysinfo::System;
use utoipa::ToSchema;

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct HealthState {
    arch: String,
    cpu_info: String,
    cpu_count: usize,
    version: Version,
}

impl Default for HealthState {
    fn default() -> Self {
        Self::new()
    }
}

impl HealthState {
    pub fn new() -> Self {
        let (cpu_info, cpu_count) = read_cpu_info();

        Self {
            arch: ARCH.to_string(),
            cpu_info,
            cpu_count,
            version: Version::new(),
        }
    }
}

pub fn read_cpu_info() -> (String, usize) {
    let mut system = System::new_all();
    system.refresh_cpu_all();
    let cpus = system.cpus();
    let count = cpus.len();
    let info = if count > 0 {
        let cpu = &cpus[0];
        cpu.brand().to_string()
    } else {
        "unknown".to_string()
    };

    (info, count)
}

#[derive(Serialize, Deserialize, ToSchema, Clone, Debug)]
pub struct Version {
    build_date: String,
    build_timestamp: String,
    git_sha: String,
    git_describe: String,
}

// TODO: used to be getting these from the env
impl Version {
    fn new() -> Self {
        Self {
            build_date: "VERGEN_BUILD_DATE".to_string(),
            build_timestamp: "VERGEN_BUILD_TIMESTAMP".to_string(),
            git_sha: "VERGEN_GIT_SHA".to_string(),
            git_describe: "VERGEN_GIT_DESCRIBE".to_string(),
        }
    }
}
