//! 開いた画像と同じフォルダの画像一覧を作る

use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use windows::core::PCWSTR;
use windows::Win32::UI::Shell::StrCmpLogicalW;

/// WIC で読める可能性がある拡張子。HEIC/AVIF/JXL などは OS に拡張機能が入っていれば読める
const EXTENSIONS: &[&str] = &[
    "jpg", "jpeg", "jpe", "jfif", "png", "gif", "bmp", "dib", "tif", "tiff", "ico", "webp", "heic",
    "heif", "avif", "jxr", "wdp", "jxl", "dds",
];

pub fn is_image(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| EXTENSIONS.iter().any(|x| x.eq_ignore_ascii_case(e)))
}

/// path が画像ならそのフォルダの画像一覧と path の位置を、フォルダならその中の画像一覧と 0 を返す。
/// サブフォルダは見ない。並びはエクスプローラーと同じ名前順（数字は数値として比較）。
pub fn list(path: &Path) -> Option<(Vec<PathBuf>, usize)> {
    let (dir, target) = if path.is_dir() {
        (path.to_path_buf(), None)
    } else {
        (path.parent()?.to_path_buf(), Some(path))
    };

    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir)
        .ok()?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_ok_and(|t| t.is_file()))
        .map(|e| e.path())
        .filter(|p| is_image(p))
        .collect();

    // 拡張子が一覧にないファイルでも、明示的に開かれたものは含める
    if let Some(t) = target {
        if !files.iter().any(|p| same_name(p, t)) {
            files.push(t.to_path_buf());
        }
    }
    if files.is_empty() {
        return None;
    }
    sort_natural(&mut files);

    let index = target
        .and_then(|t| files.iter().position(|p| same_name(p, t)))
        .unwrap_or(0);
    Some((files, index))
}

fn same_name(a: &Path, b: &Path) -> bool {
    match (a.file_name(), b.file_name()) {
        (Some(x), Some(y)) => x.to_string_lossy().to_lowercase() == y.to_string_lossy().to_lowercase(),
        _ => false,
    }
}

fn sort_natural(files: &mut Vec<PathBuf>) {
    let mut keyed: Vec<(Vec<u16>, PathBuf)> = files
        .drain(..)
        .map(|p| {
            let name: Vec<u16> = p
                .file_name()
                .unwrap_or_default()
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            (name, p)
        })
        .collect();
    keyed.sort_by(|a, b| {
        let c = unsafe { StrCmpLogicalW(PCWSTR(a.0.as_ptr()), PCWSTR(b.0.as_ptr())) };
        c.cmp(&0)
    });
    files.extend(keyed.into_iter().map(|(_, p)| p));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_order() {
        let mut v: Vec<PathBuf> = ["p10.jpg", "p2.jpg", "P1.png", "p100.jpg"].iter().map(PathBuf::from).collect();
        sort_natural(&mut v);
        let names: Vec<_> = v.iter().map(|p| p.to_str().unwrap()).collect();
        assert_eq!(names, ["P1.png", "p2.jpg", "p10.jpg", "p100.jpg"]);
    }

    #[test]
    fn extension_filter() {
        assert!(is_image(Path::new("a.JPG")));
        assert!(is_image(Path::new("a.webp")));
        assert!(!is_image(Path::new("a.txt")));
        assert!(!is_image(Path::new("jpg")));
    }

    #[test]
    fn lists_folder_and_finds_target() {
        let dir = std::env::temp_dir().join(format!("karui-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        for n in ["b10.png", "b2.png", "a.jpg", "note.txt"] {
            std::fs::write(dir.join(n), b"").unwrap();
        }
        let (files, i) = list(&dir.join("b2.png")).unwrap();
        let names: Vec<_> = files.iter().map(|p| p.file_name().unwrap().to_str().unwrap()).collect();
        assert_eq!(names, ["a.jpg", "b2.png", "b10.png"]);
        assert_eq!(i, 1);
        let (_, i) = list(&dir).unwrap();
        assert_eq!(i, 0);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
