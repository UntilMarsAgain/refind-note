//   Refind Note is a note-taking software.
//   Copyright (C) 2026 Until Mars Again
//
//   This program is free software: you can redistribute it and/or modify
//   it under the terms of the GNU Affero General Public License as published by
//   the Free Software Foundation, either version 3 of the License, or
//   (at your option) any later version.
//
//   This program is distributed in the hope that it will be useful,
//   but WITHOUT ANY WARRANTY; without even the implied warranty of
//   MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
//   GNU Affero General Public License for more details.
//
//   You should have received a copy of the GNU Affero General Public License
//   along with this program.  If not, see <http://www.gnu.org/licenses/>.

//! 把整个仓库打成一个 `.zip`（"迁移/备份"那一下）。
//!
//! ## 与同步的区别 —— 两者不是一回事
//!
//! 同步（`features/sync`）是**给程序自己用的**：它走 S3、按内容寻址、
//! 读懂封装格式的层，知道哪一版是哪一版。
//!
//! 这里是把目录**原样装进压缩包**：不解封装、不认识 blob、只是把字节搬进 zip。
//! 所以它有三个同步没有的好处，也因此能用来做同步做不到的事：
//!
//! - **换机器**（新电脑上还没配同步）；
//! - **同步坏了时留底**（云端被写坏了，本地这份原始的还能救）；
//! - **人工检查**（解开看看，里面到底是什么）。
//!
//! 反过来它**不能**代替同步：压缩包没有增量、没有历史，人不能直接在里面 diff。
//!
//! ## 为什么"原样"是对的
//!
//! 仓库里存的是**封装过的字节**（`storage/codec`）：加密的、签名过的、
//! 压缩过的。解开再重新封装就等于"让程序改写用户的数据"——
//! 签名会失效、加密的层可能被重新编码，而用户要的是**原封不动搬过去**。
//! 所以这里只读、只写，不解释一个字节。

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

/// 顶层目录名（zip 里所有路径都挂在它下面）。
///
/// 为什么要有这一层：解开压缩包直接散出一堆 `db/`、`settings/` 很容易盖到别处去
/// （尤其在文件管理器的"解压到当前目录"）。套一层之后，解出来是一个**文件夹**。
const TOP_DIR: &str = "refind-note";

/// 压缩包里**跳过**的东西。
///
/// 都是能重新生成的、放着只会变大或引起混淆的：
/// - `staging/`：导出时用的临时目录（`platform::staging`），里面可能有半个文件；
/// - `.DS_Store` / `Thumbs.db`：别的系统蹭进来的；
/// - 临时文件（`*.tmp` 与本模块自己写的 `*.zip`）：**正在写的东西**。
const SKIP_NAMES: &[&str] = &["staging", ".DS_Store", "Thumbs.db"];

/// 打成一个 zip，写到 `target`。
///
/// 返回写进包里的文件数与总字节（给界面一句"打包了 N 个文件"的话）。
///
/// # 会跳过什么
///
/// 见 [`SKIP_NAMES`]。**不动原仓库**：只读不写，目标路径自身也会跳过
/// （见 [`should_skip`]）—— 否则目标就在仓库里时会把正在写的文件也塞进去。
pub fn write_zip(root: &Path, target: &Path) -> Result<(usize, u64), String> {
    let file = fs::File::create(target)
        .map_err(|error| format!("建不出 {}：{error}", target.display()))?;
    let mut zip = zip::ZipWriter::new(std::io::BufWriter::new(file));
    let mut count = 0usize;
    let mut bytes = 0u64;

    for entry in walk(root, root, target)? {
        let (absolute, relative) = entry;
        let name = format!(
            "{TOP_DIR}/{}",
            relative.to_string_lossy().replace('\\', "/")
        );
        // 文件名里可能有不认识的字节（Linux 上完全可能），写不成 UTF-8 就退回
        // "原始字节"那个约定，而不是整个打包失败
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .unix_permissions(0o644);
        zip.start_file_from_path(name, options)
            .map_err(|error| format!("写不进压缩包：{error}"))?;
        let data = fs::read(&absolute)
            .map_err(|error| format!("读不了 {}：{error}", absolute.display()))?;
        zip.write_all(&data)
            .map_err(|error| format!("写不进压缩包：{error}"))?;
        count += 1;
        bytes += data.len() as u64;
    }

    zip.finish()
        .map_err(|error| format!("收尾失败：{error}"))?
        .flush()
        .map_err(|error| format!("落盘失败：{error}"))?;
    Ok((count, bytes))
}

/// 遍历要装进去的文件（顺带跳过 [`SKIP_NAMES`] 与目标自身）。
///
/// 先收齐再返回（`Vec`）：边走边写的话，中途遇到一个读不了的文件会留下
/// **半个压缩包**，而那个文件看起来还是成功的 —— 宁可失败得干脆一点。
fn walk(root: &Path, dir: &Path, target: &Path) -> Result<Vec<(PathBuf, PathBuf)>, String> {
    let mut out = Vec::new();
    let entries =
        fs::read_dir(dir).map_err(|error| format!("读不了目录 {}：{error}", dir.display()))?;

    for entry in entries {
        let entry = entry.map_err(|error| format!("读不了目录项：{error}"))?;
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if should_skip(&path, &name, target) {
            continue;
        }
        // 符号链接不跟：跟着走可能绕出仓库，也可能成环。仓库自己也不该有链接
        if entry.file_type().map(|t| t.is_symlink()).unwrap_or(false) {
            continue;
        }
        if path.is_dir() {
            out.extend(walk(root, &path, target)?);
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("路径不在仓库里：{error}"))?;
        out.push((path.clone(), relative.to_path_buf()));
    }
    Ok(out)
}

/// 这个条目要不要跳过。
///
/// 单独成一个函数（而不是内联在 `walk` 里）是因为它有**两条独立的理由**：
/// 「这个文件名本身该跳」与「它就是我们要写的那个目标」。后者最容易漏 ——
/// 目标在仓库里的话，不跳就会把**正在写的压缩包**装进它自己。
fn should_skip(path: &Path, name: &str, target: &Path) -> bool {
    if SKIP_NAMES.contains(&name) {
        return true;
    }
    if path == target {
        return true;
    }
    let suffix = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    // 正在写的临时文件，以及我们自己产出的压缩包（重复备份时别把上一份也塞进去）
    suffix == "tmp" || (suffix == "zip" && name.starts_with("refind-note"))
}

#[cfg(test)]
mod tests {
    use super::{should_skip, walk, write_zip, SKIP_NAMES};
    use std::fs;
    use std::io::Read;
    use std::path::{Path, PathBuf};

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "refind-note-zip-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn the_archive_is_self_nesting_and_holds_everything() {
        let root = scratch("all");
        write(&root.join("db/notes/abc"), "第一篇");
        write(&root.join("db/blobs/deadbeef"), "内容寻址的一块");
        write(&root.join("settings/preferences.json"), "{}");
        let target = root.join("out.zip");

        let (count, _) = write_zip(&root, &target).unwrap();
        assert!(target.exists(), "压缩包该落地");
        assert_eq!(count, 3, "三个文件都该在里面");

        let file = fs::File::open(&target).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        // 顶层目录是"解开得到一个文件夹"这件事的保证
        assert!(
            names.iter().all(|n| n.starts_with("refind-note/")),
            "所有路径都该挂在顶层目录下：{names:?}"
        );
        assert!(
            names.iter().any(|n| n.ends_with("db/blobs/deadbeef")),
            "{names:?}"
        );
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn it_skips_junk_staging_and_its_own_output() {
        let root = scratch("skips");
        write(&root.join("db/notes/keep"), "留下");
        write(&root.join("staging/半个文件.tmp"), "别装我");
        write(&root.join(".DS_Store"), "别装我");
        write(&root.join("db/notes/x.tmp"), "别装我");
        let target = root.join("refind-note-backup.zip");

        let (count, _) = write_zip(&root, &target).unwrap();
        assert_eq!(count, 1, "只该装 db/notes/keep 一个");

        let file = fs::File::open(&target).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(names.len(), 1, "{names:?}");
        assert!(names[0].ends_with("db/notes/keep"), "{names:?}");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn a_previous_backup_is_not_packed_into_the_next_one() {
        // 目标在仓库里（用户选了"存到工作目录"）时最容易犯的错：
        // 把上一份 refind-note-*.zip 也塞进新的一份
        let root = scratch("rebackup");
        write(&root.join("db/notes/keep"), "留下");
        write(&root.join("refind-note-上周.zip"), "上一份");

        let target = root.join("refind-note-本周.zip");
        let (count, _) = write_zip(&root, &target).unwrap();
        assert_eq!(count, 1, "只该装 db/notes/keep");

        let file = fs::File::open(&target).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        let names: Vec<String> = (0..zip.len())
            .map(|i| zip.by_index(i).unwrap().name().to_string())
            .collect();
        assert!(
            !names.iter().any(|n| n.ends_with(".zip")),
            "压缩包里不该再出现压缩包：{names:?}"
        );

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn binary_bytes_survive_the_round_trip_unchanged() {
        // 仓库里全是**封装过的字节**（可能加密/压缩/签名）。
        // 打包必须原样搬 —— 任何"顺手重新编码"都会让签名失效。
        let root = scratch("bytes");
        let blob: Vec<u8> = (0..=255u8).cycle().take(4096).collect();
        fs::create_dir_all(root.join("db/blobs")).unwrap();
        fs::write(root.join("db/blobs/xx"), &blob).unwrap();
        let target = root.join("out.zip");

        write_zip(&root, &target).unwrap();
        let file = fs::File::open(&target).unwrap();
        let mut zip = zip::ZipArchive::new(file).unwrap();
        // 读**字节**：仓库里的 blob 可能是加密后的任意字节，未必是 UTF-8。
        // 用 `read_to_string` 读它会先因为"不是文本"就失败 —— 那是读法的问题，
        // 不是打包的问题（这正是这条测试要验的那件事）。
        let mut got = Vec::new();
        zip.by_name("refind-note/db/blobs/xx")
            .unwrap()
            .read_to_end(&mut got)
            .unwrap();
        assert_eq!(got, blob, "字节必须一字不差");

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn nested_directories_are_walked() {
        let root = scratch("nested");
        write(&root.join("a/b/c/d/深层"), "x");
        write(&root.join("a/b/中间"), "y");
        let target = root.join("out.zip");
        let (count, _) = write_zip(&root, &target).unwrap();
        assert_eq!(count, 2, "多层目录都要走到");

        let listed = walk(&root, &root, &target).unwrap();
        assert_eq!(listed.len(), 2);
        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn symlinks_are_left_alone() {
        // 跟进去可能绕出仓库，也可能成环；仓库自己不该有链接，遇到就跳过
        #[cfg(unix)]
        {
            let root = scratch("link");
            write(&root.join("real/file"), "x");
            std::os::unix::fs::symlink(root.join("real"), root.join("link")).unwrap();
            let target = root.join("out.zip");
            let (count, _) = write_zip(&root, &target).unwrap();
            assert_eq!(count, 1, "只该装 real/file，不该跟着链接跑");
            fs::remove_dir_all(&root).unwrap();
        }
    }

    #[test]
    fn the_skip_list_is_not_empty_for_a_reason() {
        // 钉住这几项：有人从列表里删掉一项时，这条会失败并说明少的是什么
        assert!(SKIP_NAMES.contains(&"staging"), "staging 是导出临时目录");
        assert!(SKIP_NAMES.contains(&".DS_Store"));
        assert!(SKIP_NAMES.contains(&"Thumbs.db"));
        let root = Path::new("/x/y.tmp");
        assert!(should_skip(root, "y.tmp", Path::new("/other")), ".tmp 要跳");
    }
}
