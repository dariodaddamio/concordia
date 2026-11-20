//! Conservative build parallelism defaults. Explicit user/provider settings take precedence.
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineResources {
    pub logical_cores: u32,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    pub build_jobs: usize,
}

pub fn budget(m: MachineResources) -> ResourceBudget {
    ResourceBudget {
        build_jobs: m.logical_cores.clamp(1, 4) as usize,
    }
}

pub fn default_build_env(
    jobs: usize,
    inherited: impl IntoIterator<Item = String>,
) -> Vec<(String, String)> {
    let inherited: HashSet<String> = inherited.into_iter().collect();
    [
        ("CARGO_BUILD_JOBS", jobs.to_string()),
        ("MAKEFLAGS", format!("-j{jobs}")),
        ("npm_config_jobs", jobs.to_string()),
        ("CMAKE_BUILD_PARALLEL_LEVEL", jobs.to_string()),
    ]
    .into_iter()
    .filter(|(k, _)| {
        #[cfg(windows)]
        let explicit = inherited.iter().any(|name| name.eq_ignore_ascii_case(k));
        #[cfg(not(windows))]
        let explicit = inherited.contains(*k);
        !explicit && std::env::var_os(k).is_none()
    })
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn build_job_defaults_respect_reported_core_count() {
        for (logical_cores, build_jobs) in [(0, 1), (2, 2), (16, 4)] {
            assert_eq!(
                budget(MachineResources { logical_cores }),
                ResourceBudget { build_jobs }
            );
        }
    }
}
