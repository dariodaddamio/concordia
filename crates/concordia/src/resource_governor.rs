//! Deterministic resource budgeting for delegated workers and build children.
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MachineResources {
    pub total_ram_bytes: u64,
    pub free_ram_bytes: u64,
    pub logical_cores: u32,
    pub free_disk_bytes: u64,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ResourceBudget {
    pub max_workers: usize,
    pub build_jobs: usize,
}

pub fn budget(m: MachineResources) -> ResourceBudget {
    let by_ram = (m.free_ram_bytes / (8 * 1024 * 1024 * 1024)) as usize;
    let max_workers = by_ram.max(1).min((m.logical_cores / 4).max(1) as usize);
    ResourceBudget {
        max_workers,
        build_jobs: (m.logical_cores as usize / max_workers).clamp(1, 4),
    }
}
pub fn low_resource(m: MachineResources) -> bool {
    (m.total_ram_bytes > 0 && m.free_ram_bytes.saturating_mul(10) < m.total_ram_bytes)
        || m.free_disk_bytes < 5 * 1024 * 1024 * 1024
}

#[derive(Debug, Default)]
pub struct WarningEpisode {
    active: bool,
}
impl WarningEpisode {
    pub fn observe(&mut self, low: bool) -> bool {
        let fire = low && !self.active;
        self.active = low;
        fire
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
    .filter(|(k, _)| !inherited.contains(*k) && std::env::var_os(k).is_none())
    .map(|(k, v)| (k.to_string(), v))
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    const GB: u64 = 1024 * 1024 * 1024;
    fn m(r: u64) -> MachineResources {
        MachineResources {
            total_ram_bytes: r * GB,
            free_ram_bytes: r * GB,
            logical_cores: 16,
            free_disk_bytes: 100 * GB,
        }
    }
    #[test]
    fn fixture_machine_profiles_have_stable_budgets() {
        assert_eq!(
            budget(m(8)),
            ResourceBudget {
                max_workers: 1,
                build_jobs: 4
            }
        );
        assert_eq!(
            budget(m(16)),
            ResourceBudget {
                max_workers: 2,
                build_jobs: 4
            }
        );
        assert_eq!(
            budget(m(32)),
            ResourceBudget {
                max_workers: 4,
                build_jobs: 4
            }
        );
    }
    #[test]
    fn low_resource_warning_fires_once_per_episode() {
        let mut e = WarningEpisode::default();
        assert!(e.observe(true));
        assert!(!e.observe(true));
        assert!(!e.observe(false));
        assert!(e.observe(true));
    }
}
