//! workspace 边界：把工具路径解析到根内，拒绝绝对路径、点段与符号链接逃逸。
//! 注册 workspace 时的目录校验也在 `check_workspace`：文件系统事实只有 host 判定。
//! 各工具的参数校验在自己的模块，工具定义在 `durable::tool`。

use std::path::{Component, Path, PathBuf};

use durable::link::WorkspaceCheckError as WorkspaceCheckFailure;
use durable::{ToolErrorCode, ToolFailure};

/// 校验一个绝对目录能否成为 workspace 身份：必须存在、是目录、规范化后仍是 UTF-8。
///
/// 成功时返回规范化后的绝对路径：workspace 身份是 host 与它这一对，agent 只保存结果。
/// 失败类别与 `durable::WorkspacePathError` 的变体一一对应，agent 据此映射协议错误码。
pub(crate) fn check_workspace(raw: &str) -> Result<String, WorkspaceCheckFailure> {
    let path = Path::new(raw);
    if raw.is_empty() || !path.is_absolute() {
        return Err(WorkspaceCheckFailure::NotAbsolute);
    }
    let canonical = std::fs::canonicalize(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => WorkspaceCheckFailure::NotFound,
        _ => WorkspaceCheckFailure::Unreadable,
    })?;
    if !canonical.is_dir() {
        return Err(WorkspaceCheckFailure::NotDirectory);
    }
    let canonical = canonical.to_str().ok_or(WorkspaceCheckFailure::NotUtf8)?;
    Ok(canonical.to_owned())
}

/// workspace 根内的路径解析。根是已规范化的绝对路径，来自 session 的 workspace。
pub(crate) struct Workspace {
    root: PathBuf,
}

impl Workspace {
    pub(crate) fn new(root: &str) -> Self {
        Self {
            root: PathBuf::from(root),
        }
    }

    pub(crate) fn root(&self) -> &Path {
        &self.root
    }

    /// 解析必须存在的路径：目标与沿途符号链接都被规范化，结果必须仍在根内。
    ///
    /// 失败：路径为空或越界、目标不存在、符号链接目标非法、无法读取。
    pub(crate) fn resolve_existing(&self, raw: &str) -> Result<PathBuf, ToolFailure> {
        let candidate = self.root.join(relative(raw)?);
        let canonical = std::fs::canonicalize(&candidate).map_err(|error| match error.kind() {
            std::io::ErrorKind::NotFound => {
                ToolFailure::new(ToolErrorCode::FileNotFound, format!("文件不存在: {raw}"))
            }
            _ => ToolFailure::io(&format!("解析路径 {raw}"), &error),
        })?;
        self.contain(&canonical, raw)?;
        Ok(canonical)
    }

    /// 解析写入目标：父目录必须存在且位于根内，目标本身可以不存在。
    ///
    /// 目标已存在时校验其规范化结果；目标是悬空符号链接时拒绝。
    ///
    /// 失败：路径为空或越界、父目录不存在、符号链接目标非法。
    pub(crate) fn resolve_for_write(&self, raw: &str) -> Result<PathBuf, ToolFailure> {
        let candidate = self.root.join(relative(raw)?);
        let file_name = candidate.file_name().ok_or_else(|| {
            ToolFailure::new(
                ToolErrorCode::InvalidArguments,
                format!("路径缺少文件名: {raw}"),
            )
        })?;
        let parent = candidate.parent().ok_or_else(|| {
            ToolFailure::new(
                ToolErrorCode::InvalidArguments,
                format!("路径缺少父目录: {raw}"),
            )
        })?;
        let canonical_parent =
            std::fs::canonicalize(parent).map_err(|error| match error.kind() {
                std::io::ErrorKind::NotFound => ToolFailure::new(
                    ToolErrorCode::ParentNotFound,
                    format!("父目录不存在: {raw}"),
                ),
                _ => ToolFailure::io(&format!("解析父目录 {raw}"), &error),
            })?;
        self.contain(&canonical_parent, raw)?;
        let target = canonical_parent.join(file_name);
        if std::fs::symlink_metadata(&target).is_ok() {
            let canonical = std::fs::canonicalize(&target).map_err(|_| {
                ToolFailure::new(
                    ToolErrorCode::PathEscape,
                    format!("符号链接目标无法解析: {raw}"),
                )
            })?;
            self.contain(&canonical, raw)?;
        }
        Ok(target)
    }

    fn contain(&self, canonical: &Path, raw: &str) -> Result<(), ToolFailure> {
        if canonical.starts_with(&self.root) {
            Ok(())
        } else {
            Err(ToolFailure::new(
                ToolErrorCode::PathEscape,
                format!("路径逃逸 workspace 根: {raw}"),
            ))
        }
    }
}

/// 把模型给出的相对路径转为可拼接的部分，拒绝绝对路径与 `..`。
fn relative(raw: &str) -> Result<PathBuf, ToolFailure> {
    if raw.is_empty() {
        return Err(ToolFailure::new(
            ToolErrorCode::InvalidArguments,
            "路径不能为空",
        ));
    }
    let path = Path::new(raw);
    if path.is_absolute() {
        return Err(ToolFailure::new(
            ToolErrorCode::PathEscape,
            format!("拒绝绝对路径: {raw}"),
        ));
    }
    for component in path.components() {
        match component {
            Component::Normal(_) | Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(ToolFailure::new(
                    ToolErrorCode::PathEscape,
                    format!("拒绝逃逸路径: {raw}"),
                ));
            }
        }
    }
    Ok(path.to_path_buf())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace() -> (tempfile::TempDir, Workspace) {
        let directory = tempfile::tempdir().expect("创建临时目录");
        let root = std::fs::canonicalize(directory.path()).expect("规范化根");
        let workspace = Workspace::new(root.to_str().expect("根是 UTF-8"));
        (directory, workspace)
    }

    #[test]
    fn rejects_empty_absolute_and_parent_paths() {
        let (_directory, workspace) = workspace();
        for raw in ["", "/etc/passwd", "../escape.txt", "sub/../../escape.txt"] {
            let failure = workspace.resolve_existing(raw).expect_err("逃逸路径被拒绝");
            assert!(
                matches!(
                    failure.code,
                    ToolErrorCode::InvalidArguments | ToolErrorCode::PathEscape
                ),
                "{raw} 得到 {:?}",
                failure.code
            );
        }
    }

    #[test]
    fn resolves_paths_inside_the_root() {
        let (directory, workspace) = workspace();
        std::fs::write(directory.path().join("notes.txt"), "内容").expect("写入文件");
        let resolved = workspace.resolve_existing("./notes.txt").expect("解析路径");
        assert_eq!(
            resolved,
            std::fs::canonicalize(directory.path())
                .unwrap()
                .join("notes.txt")
        );
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_target_outside_the_root() {
        let (directory, workspace) = workspace();
        let outside = tempfile::tempdir().expect("创建外部目录");
        std::fs::write(outside.path().join("secret.txt"), "秘密").expect("写入外部文件");
        std::os::unix::fs::symlink(outside.path(), directory.path().join("link"))
            .expect("创建符号链接");

        let failure = workspace
            .resolve_existing("link/secret.txt")
            .expect_err("符号链接逃逸被拒绝");
        assert_eq!(failure.code, ToolErrorCode::PathEscape);
        let failure = workspace
            .resolve_for_write("link/new.txt")
            .expect_err("写入符号链接逃逸被拒绝");
        assert_eq!(failure.code, ToolErrorCode::PathEscape);
    }
}
