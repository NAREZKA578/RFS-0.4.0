//! Атомарная запись JSON-файлов (tmp + rename).
//!
//! Донор: `RFS-0.3/src/game/save_load.rs:135-150` (saves, уже атомарно — №151),
//! `career.rs` (№48) — против `screens/server_browser.rs:230-241` (favorites,
//! прямой `fs::write`).
//!
//! Ошибка донора (№224 FAV-ATOMIC-1): обрыв процесса ронял обрезанный
//! `favorites.json`, следующий старт молча отдавал пустой сет.
//! Здесь — единый хелпер для любых JSON-файлов движка.

use std::path::Path;

/// Атомарно пишет `data` в `path`: сначала tmp-файл рядом, потом rename.
/// Битая запись невозможна: читатели видят либо старый целый файл, либо новый.
pub fn atomic_write(path: &Path, data: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn atomic_write_roundtrip_regression_224() {
        let dir = std::env::temp_dir().join("rfs-engine-test-persist");
        let path = dir.join("fav.json");
        let _ = std::fs::remove_dir_all(&dir);
        atomic_write(&path, br#"{"a":1}"#).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), br#"{"a":1}"#);
        // Tmp-файла после успеха оставаться не должно.
        assert!(!path.with_extension("json.tmp").exists());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
