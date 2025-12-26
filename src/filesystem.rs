//! 文件系统辅助工具
//!
//! - 异步读取目录并收集候选文件
//! - 依据扩展名建立文件基名到路径的映射
//! - 适配 BMS 资源在不同目录的实际文件名

use std::{path::Path, path::PathBuf};

use anyhow::Result;
use async_fs as afs;
use futures_lite::{StreamExt, stream};
use std::collections::{HashMap, HashSet};

/// 在父目录及子目录中按扩展名查找文件，返回“文件基名->路径”的映射
///
/// - `parent`：父目录
/// - `children`：子路径列表（仅用于确定可能的子目录）
/// - `exts`：允许的扩展名集合（不区分大小写）
/// - 若同名文件存在多个，仅保留首次发现的路径
pub async fn choose_paths_by_ext_async(
    parent: &Path,
    children: &[PathBuf],
    exts: &[&str],
) -> HashMap<String, PathBuf> {
    let dirs: HashSet<PathBuf> = std::iter::once(parent.to_path_buf())
        .chain(
            children
                .iter()
                .map(|c| parent.join(c))
                .map(|p| p.parent().unwrap_or(parent).to_path_buf()),
        )
        .collect();

    let mut entries: Vec<(String, String, PathBuf)> = Vec::new();
    for dir_path in dirs {
        let Ok(mut dir) = afs::read_dir(&dir_path).await else {
            continue;
        };
        let raw: Vec<Result<afs::DirEntry, std::io::Error>> = StreamExt::collect(&mut dir).await;
        let Ok(items) = raw.into_iter().collect::<Result<Vec<_>, _>>() else {
            continue;
        };
        let collected: Vec<Option<(String, String, PathBuf)>> = stream::iter(items)
            .then(|entry| async move {
                let Ok(ft) = entry.file_type().await else {
                    return None;
                };
                if !ft.is_file() {
                    return None;
                }
                let p = entry.path();
                let stem = p.file_stem().and_then(|s| s.to_str()).map(str::to_string)?;
                let ext = p.extension().and_then(|s| s.to_str()).map(str::to_string)?;
                Some((stem, ext, p))
            })
            .collect()
            .await;
        entries.extend(collected.into_iter().flatten());
    }

    let mut found: HashMap<String, PathBuf> = HashMap::new();
    for (stem, e, p) in entries.into_iter() {
        if exts.iter().any(|x| e.eq_ignore_ascii_case(x)) {
            found.entry(stem).or_insert(p);
        }
    }
    found
}
