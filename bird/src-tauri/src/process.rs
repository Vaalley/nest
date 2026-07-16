//! Process-monitoring backends for the Feather Agent.
//!
//! Phase 8 targets Windows first but keeps a [`ProcessBackend`] trait so a
//! Linux / SteamOS / macOS backend can be swapped in later. The default
//! Windows MVP implementation is built on top of the `sysinfo` crate.

use std::path::{Path, PathBuf};

use crate::error::BirdResult;

/// Lightweight snapshot of a running process.
#[derive(Debug, Clone)]
pub struct ProcessInfo {
    pub pid: u32,
    pub name: String,
    pub exe: Option<PathBuf>,
}

/// Platform-agnostic interface used by the Feather Agent to detect game
/// launches and track PIDs.
pub trait ProcessBackend: Send + Sync {
    /// Refresh the process list.
    fn refresh(&mut self);

    /// Return true if the process with `pid` still exists.
    fn is_running(&self, pid: u32) -> bool;

    /// Find the first process whose name or executable path matches one of
    /// `names`. Matching is case-insensitive on Windows and tolerant of
    /// missing `.exe` extensions.
    fn find_by_name(&self, names: &[String]) -> Option<ProcessInfo>;
}

/// Windows MVP process backend using `sysinfo`.
pub struct SysinfoBackend {
    system: sysinfo::System,
}

impl SysinfoBackend {
    pub fn new() -> BirdResult<Self> {
        let system = sysinfo::System::new_with_specifics(
            sysinfo::RefreshKind::new().with_processes(sysinfo::ProcessRefreshKind::everything()),
        );
        Ok(Self { system })
    }
}

impl ProcessBackend for SysinfoBackend {
    fn refresh(&mut self) {
        self.system.refresh_processes();
    }

    fn is_running(&self, pid: u32) -> bool {
        self.system.process(sysinfo::Pid::from_u32(pid)).is_some()
    }

    fn find_by_name(&self, names: &[String]) -> Option<ProcessInfo> {
        let targets: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
        let target_stems: Vec<String> = targets
            .iter()
            .map(|n| {
                if n.ends_with(".exe") {
                    n[..n.len() - 4].to_string()
                } else {
                    n.clone()
                }
            })
            .collect();

        for (pid, proc) in self.system.processes() {
            let pid = pid.as_u32();
            let proc_name = Path::new(proc.name())
                .file_stem()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();
            let proc_name_full = Path::new(proc.name())
                .file_name()
                .map(|s| s.to_string_lossy().to_lowercase())
                .unwrap_or_default();

            let exe_name = proc
                .exe()
                .and_then(|p| p.file_name())
                .map(|s| s.to_string_lossy().to_lowercase());
            let exe_stem = proc
                .exe()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string_lossy().to_lowercase());

            if targets.iter().any(|t| {
                proc_name == *t
                    || proc_name_full == *t
                    || exe_name.as_deref() == Some(t)
                    || exe_stem.as_deref() == Some(t)
            }) || target_stems
                .iter()
                .any(|t| proc_name == *t || proc_name_full == *t || exe_stem.as_deref() == Some(t))
            {
                return Some(ProcessInfo {
                    pid,
                    name: Path::new(proc.name())
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_else(|| proc_name.clone()),
                    exe: proc.exe().map(Path::to_path_buf),
                });
            }
        }

        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct FakeBackend {
        processes: Vec<ProcessInfo>,
    }

    impl ProcessBackend for FakeBackend {
        fn refresh(&mut self) {}
        fn is_running(&self, pid: u32) -> bool {
            self.processes.iter().any(|p| p.pid == pid)
        }
        fn find_by_name(&self, names: &[String]) -> Option<ProcessInfo> {
            let targets: Vec<String> = names.iter().map(|n| n.to_lowercase()).collect();
            self.processes
                .iter()
                .find(|p| {
                    let p_name = p.name.to_lowercase();
                    let p_exe = p
                        .exe
                        .as_ref()
                        .and_then(|e| e.file_name())
                        .map(|s| s.to_string_lossy().to_lowercase());
                    targets.iter().any(|t| {
                        p_name == *t
                            || p_name.trim_end_matches(".exe") == t.trim_end_matches(".exe")
                            || p_exe.as_deref() == Some(t)
                    })
                })
                .cloned()
        }
    }

    #[test]
    fn case_insensitive_match_with_exe_extension() {
        let backend = FakeBackend {
            processes: vec![ProcessInfo {
                pid: 1234,
                name: "Stardew Valley.exe".to_string(),
                exe: Some(PathBuf::from("C:/games/Stardew Valley.exe")),
            }],
        };
        assert!(backend
            .find_by_name(&["stardew valley".to_string()])
            .is_some());
        assert!(backend
            .find_by_name(&["Stardew Valley.exe".to_string()])
            .is_some());
    }
}
