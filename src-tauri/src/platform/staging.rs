//! 交给系统默认应用打开时，先把内容落到一个临时文件上。
//!
//! 仓库里存的是字节，没有一个"能在文件管理器里双击"的路径 —— 所以这一步是必须的。
//! 落在临时目录里是**刻意**的：它是给外部程序看的副本，用完由系统回收，
//! 不进仓库、也不该被当成原件的家。加密存的内容到了这一步已经是明文，
//! 所以权限收紧到只有本人可读。

/// 把一个文件的字节落到临时目录里，返回那个路径。
///
/// 名字取页面名（后缀要留着：系统靠它挑应用）。**权限收紧到只有本人可读** ——
/// 从仓库里出来的可能是解过密的明文，临时目录别的人也可能看得到。
pub fn stage_file(name: &str, bytes: &[u8]) -> Result<std::path::PathBuf, String> {
    let directory = std::env::temp_dir().join("refind-note-open");
    std::fs::create_dir_all(&directory)
        .map_err(|error| format!("建不出临时目录 {}：{error}", directory.display()))?;
    let path = directory.join(name);
    crate::storage::workspace::write_bytes(&path, bytes)?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))
            .map_err(|error| format!("改权限失败：{error}"))?;
    }
    Ok(path)
}
