use anyhow::Result;
use log::debug;
use serde::{Deserialize, Serialize};
use sysinfo::{ProcessesToUpdate, System};

const MB: u64 = 1024 * 1024;

// New tiered memory thresholds
const MEMORY_GREEN_MAX: u64 = 150;      // <150MB green
const MEMORY_YELLOW_MAX: u64 = 250;     // 150-250MB yellow
const MEMORY_ORANGE_MAX: u64 = 400;     // 250-400MB orange
// >400MB red

const MEMORY_LIMIT_MB: u64 = 400;       // Hard limit
const WARNING_THRESHOLD_MB: u64 = 250;  // Yellow warning

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryInfo {
    /// Current memory usage in MB
    pub used_mb: f64,
    /// Memory limit in MB
    pub limit_mb: u64,
    /// Warning threshold in MB
    pub warning_threshold_mb: u64,
    /// Whether memory usage is approaching the limit
    pub is_approaching_limit: bool,
    /// Whether memory usage has exceeded the limit
    pub is_over_limit: bool,
    /// Percentage of limit being used
    pub percentage_used: f64,
    /// Memory level: "green", "yellow", "orange", or "red"
    pub level: String,
}

/// Tracks memory usage for the Rust backend process only.
///
/// **Production note:** In a bundled Tauri app, the webview (rendering engine) runs
/// as a separate OS process. `sysinfo::Process::memory()` only reports the Rust
/// process's resident set size, so the webview's memory is *not* included in these
/// figures. Aggregating child-process memory is platform-specific and fragile, so
/// we intentionally scope this to the backend process and document the limitation
/// in the UI (MemoryDetailsModal).
pub struct MemoryService {
    system: System,
}

impl MemoryService {
    pub fn new() -> Self {
        Self {
            system: System::new_all(),
        }
    }

    /// Get current memory usage information
    pub fn get_memory_info(&mut self) -> Result<MemoryInfo> {
        // Get current process PID
        let current_pid = sysinfo::get_current_pid()
            .map_err(|e| anyhow::anyhow!("Failed to get current PID: {}", e))?;

        // Refresh this specific process to get accurate memory info
        self.system.refresh_processes(ProcessesToUpdate::Some(&[current_pid]), true);

        let process = self.system.process(current_pid)
            .ok_or_else(|| anyhow::anyhow!("Failed to get process info"))?;

        // Get memory usage in bytes and convert to MB
        let used_bytes = process.memory();
        let info = build_memory_info(used_bytes);

        debug!("Memory Usage: {:.2} MB / {} MB ({:.1}%) - {}",
                 info.used_mb, MEMORY_LIMIT_MB, info.percentage_used, info.level);

        Ok(info)
    }
}

/// Classify a memory reading (in MB) into one of the tiered levels.
fn classify_memory_level(used_mb: f64) -> &'static str {
    if used_mb < MEMORY_GREEN_MAX as f64 {
        "green"
    } else if used_mb < MEMORY_YELLOW_MAX as f64 {
        "yellow"
    } else if used_mb < MEMORY_ORANGE_MAX as f64 {
        "orange"
    } else {
        "red"
    }
}

/// Build a `MemoryInfo` report from a raw byte count. Pure; no system access.
fn build_memory_info(used_bytes: u64) -> MemoryInfo {
    let used_mb = used_bytes as f64 / MB as f64;

    let percentage_used = (used_mb / MEMORY_LIMIT_MB as f64) * 100.0;
    let is_approaching_limit = used_mb >= WARNING_THRESHOLD_MB as f64;
    let is_over_limit = used_mb >= MEMORY_LIMIT_MB as f64;

    MemoryInfo {
        used_mb,
        limit_mb: MEMORY_LIMIT_MB,
        warning_threshold_mb: WARNING_THRESHOLD_MB,
        is_approaching_limit,
        is_over_limit,
        percentage_used,
        level: classify_memory_level(used_mb).to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn memory_level_thresholds_are_exclusive_upper_bounds() {
        assert_eq!(classify_memory_level(0.0), "green");
        assert_eq!(classify_memory_level(149.999), "green");
        assert_eq!(classify_memory_level(150.0), "yellow");
        assert_eq!(classify_memory_level(249.999), "yellow");
        assert_eq!(classify_memory_level(250.0), "orange");
        assert_eq!(classify_memory_level(399.999), "orange");
        assert_eq!(classify_memory_level(400.0), "red");
        assert_eq!(classify_memory_level(10_000.0), "red");
    }

    #[test]
    fn memory_info_converts_bytes_to_megabytes() {
        let info = build_memory_info(100 * MB);
        assert!((info.used_mb - 100.0).abs() < f64::EPSILON);
        assert!((info.percentage_used - 25.0).abs() < 1e-9);
        assert_eq!(info.limit_mb, MEMORY_LIMIT_MB);
        assert_eq!(info.warning_threshold_mb, WARNING_THRESHOLD_MB);
    }

    #[test]
    fn memory_info_flags_are_consistent_with_thresholds() {
        let low = build_memory_info(100 * MB);
        assert!(!low.is_approaching_limit);
        assert!(!low.is_over_limit);
        assert_eq!(low.level, "green");

        let warning = build_memory_info(WARNING_THRESHOLD_MB * MB);
        assert!(warning.is_approaching_limit);
        assert!(!warning.is_over_limit);
        assert_eq!(warning.level, "orange");

        let over = build_memory_info(MEMORY_LIMIT_MB * MB);
        assert!(over.is_approaching_limit);
        assert!(over.is_over_limit);
        assert_eq!(over.level, "red");
        assert!((over.percentage_used - 100.0).abs() < 1e-9);
    }

    #[test]
    fn memory_service_reports_current_process_without_error() {
        let mut service = MemoryService::new();
        let info = service.get_memory_info().expect("current process should be readable");
        assert!(info.used_mb > 0.0);
        assert!(["green", "yellow", "orange", "red"].contains(&info.level.as_str()));
    }
}
