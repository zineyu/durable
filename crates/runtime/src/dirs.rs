//! 应用的标准目录。
//!
//! 目录规则跟随各平台惯例：macOS 用 `Library/Application Support` 与
//! `Library/Logs`；其余类 Unix 平台用 XDG 基础目录规范；Windows 用
//! `LOCALAPPDATA`。环境变量经 [`Env`] 注入，因此解析结果可测试。

use std::env;
use std::path::PathBuf;

use thiserror::Error;

/// 应用在标准目录中的名称。
pub const APP_NAME: &str = "durable";

/// 应用的默认目录：可写数据与日志。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppDirs {
    pub data: PathBuf,
    pub logs: PathBuf,
}

/// 目录解析读取的环境变量。空值按未设置处理。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Env {
    pub home: Option<PathBuf>,
    pub xdg_data_home: Option<PathBuf>,
    pub xdg_state_home: Option<PathBuf>,
    pub local_app_data: Option<PathBuf>,
}

impl Env {
    /// 读取当前进程的环境。
    pub fn from_process() -> Self {
        Self {
            home: read("HOME"),
            xdg_data_home: read("XDG_DATA_HOME"),
            xdg_state_home: read("XDG_STATE_HOME"),
            local_app_data: read("LOCALAPPDATA"),
        }
    }
}

fn read(name: &str) -> Option<PathBuf> {
    env::var_os(name)
        .map(PathBuf::from)
        .filter(|path| !path.as_os_str().is_empty())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum DirError {
    #[error("无法确定标准目录：环境变量 HOME 未设置")]
    MissingHome,
    #[error("无法确定标准目录：环境变量 LOCALAPPDATA 与 HOME 均未设置")]
    MissingLocalAppData,
}

/// 按当前进程环境解析标准目录。
///
/// 失败：所需的基础环境变量缺失。
pub fn app_dirs() -> Result<AppDirs, DirError> {
    resolve(&Env::from_process())
}

#[cfg(target_os = "macos")]
pub fn resolve(env: &Env) -> Result<AppDirs, DirError> {
    let home = env.home.as_ref().ok_or(DirError::MissingHome)?;
    Ok(AppDirs {
        data: home
            .join("Library")
            .join("Application Support")
            .join(APP_NAME),
        logs: home.join("Library").join("Logs").join(APP_NAME),
    })
}

#[cfg(windows)]
pub fn resolve(env: &Env) -> Result<AppDirs, DirError> {
    let base = match &env.local_app_data {
        Some(path) => path.clone(),
        None => env
            .home
            .as_ref()
            .ok_or(DirError::MissingLocalAppData)?
            .join("AppData")
            .join("Local"),
    };
    Ok(AppDirs {
        data: base.join(APP_NAME),
        logs: base.join(APP_NAME).join("logs"),
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
pub fn resolve(env: &Env) -> Result<AppDirs, DirError> {
    let data = match &env.xdg_data_home {
        Some(path) => path.join(APP_NAME),
        None => env
            .home
            .as_ref()
            .ok_or(DirError::MissingHome)?
            .join(".local")
            .join("share")
            .join(APP_NAME),
    };
    let logs = match &env.xdg_state_home {
        Some(path) => path.join(APP_NAME).join("log"),
        None => env
            .home
            .as_ref()
            .ok_or(DirError::MissingHome)?
            .join(".local")
            .join("state")
            .join(APP_NAME)
            .join("log"),
    };
    Ok(AppDirs { data, logs })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_uses_application_support_and_logs() {
        let env = Env {
            home: Some(PathBuf::from("/Users/example")),
            ..Env::default()
        };
        let dirs = resolve(&env).expect("解析目录");
        assert_eq!(
            dirs.data,
            PathBuf::from("/Users/example/Library/Application Support/durable")
        );
        assert_eq!(
            dirs.logs,
            PathBuf::from("/Users/example/Library/Logs/durable")
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn macos_requires_home() {
        assert_eq!(resolve(&Env::default()), Err(DirError::MissingHome));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn unix_honors_xdg_base_directories() {
        let env = Env {
            home: Some(PathBuf::from("/home/example")),
            xdg_data_home: Some(PathBuf::from("/xdg/data")),
            xdg_state_home: Some(PathBuf::from("/xdg/state")),
            ..Env::default()
        };
        let dirs = resolve(&env).expect("解析目录");
        assert_eq!(dirs.data, PathBuf::from("/xdg/data/durable"));
        assert_eq!(dirs.logs, PathBuf::from("/xdg/state/durable/log"));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn unix_falls_back_to_home_without_xdg_variables() {
        let env = Env {
            home: Some(PathBuf::from("/home/example")),
            ..Env::default()
        };
        let dirs = resolve(&env).expect("解析目录");
        assert_eq!(
            dirs.data,
            PathBuf::from("/home/example/.local/share/durable")
        );
        assert_eq!(
            dirs.logs,
            PathBuf::from("/home/example/.local/state/durable/log")
        );
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn unix_requires_home_without_xdg_variables() {
        assert_eq!(resolve(&Env::default()), Err(DirError::MissingHome));
    }

    #[cfg(not(any(target_os = "macos", windows)))]
    #[test]
    fn unix_accepts_xdg_variables_without_home() {
        let env = Env {
            xdg_data_home: Some(PathBuf::from("/xdg/data")),
            xdg_state_home: Some(PathBuf::from("/xdg/state")),
            ..Env::default()
        };
        let dirs = resolve(&env).expect("解析目录");
        assert_eq!(dirs.data, PathBuf::from("/xdg/data/durable"));
        assert_eq!(dirs.logs, PathBuf::from("/xdg/state/durable/log"));
    }

    #[cfg(windows)]
    #[test]
    fn windows_uses_local_app_data() {
        let env = Env {
            local_app_data: Some(PathBuf::from("C:/Users/example/AppData/Local")),
            ..Env::default()
        };
        let dirs = resolve(&env).expect("解析目录");
        assert_eq!(
            dirs.data,
            PathBuf::from("C:/Users/example/AppData/Local/durable")
        );
        assert_eq!(
            dirs.logs,
            PathBuf::from("C:/Users/example/AppData/Local/durable/logs")
        );
    }
}
