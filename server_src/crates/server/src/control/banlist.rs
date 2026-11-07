//! Бан-лист (этап 3, план `server-interface-plan.md`, раздел 6.2/9).
//! Хранит идентификаторы, которым запрещён вход на сервер: Steam ID (после
//! входа через Steam) или сетевой адрес (до Steam). Файл лежит рядом с
//! конфигом, переживает перезапуск, читается при подключении каждого игрока.
//! Токены управления и секреты игроков никогда не попадают в бан-лист.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Запись бан-листа: цель (Steam ID или адрес), причина, кто и когда выдал бан.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BanEntry {
    pub target: String,
    pub reason: String,
    pub banned_by: String,
    pub banned_at: String,
}

/// Бан-лист: цель → запись бан. HashMap даёт O(1) проверку при входе.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BanList {
    pub banned: HashMap<String, BanEntry>,
}

fn format_now() -> String {
    let wall = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = wall.as_secs();
    let millis = wall.subsec_millis();
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (y, mo, d) = civil_from_days(days);
    let (h, mi, s) = (rem / 3_600, (rem % 3_600) / 60, rem % 60);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}.{millis:03}Z")
}

/// Count of whole days since 1970-01-01 → a (year, month, day) UTC triple.
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m as u32, d)
}

impl BanList {
    /// Загружает бан-лист из файла. Если файла нет — создаёт пустой.
    pub fn load(path: &Path) -> Result<Self> {
        if !path.exists() {
            let empty = Self {
                banned: HashMap::new(),
            };
            empty.save(path).with_context(|| {
                format!("creating the ban list file {}", path.display())
            })?;
            return Ok(empty);
        }
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("reading the ban list from {}", path.display()))?;
        let list: Self =
            serde_json::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(list)
    }

    /// Сохраняет бан-лист в файл.
    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() && !parent.exists() {
                std::fs::create_dir_all(parent)
                    .with_context(|| format!("creating {}", parent.display()))?;
            }
        }
        let text = serde_json::to_string_pretty(self)
            .context("serializing the ban list")?;
        std::fs::write(path, text)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    /// `true` когда цель находится в бан-листе.
    pub fn is_banned(&self, target: &str) -> bool {
        self.banned.contains_key(target)
    }

    /// Добавляет (или обновляет) запись бан. Возвращает `true` когда это
    /// новый бан, `false` когда цель уже была забанена (запись обновляется).
    pub fn ban(&mut self, target: String, reason: String, banned_by: String) -> bool {
        let is_new = !self.banned.contains_key(&target);
        self.banned.insert(
            target.clone(),
            BanEntry {
                target,
                reason,
                banned_by,
                banned_at: format_now(),
            },
        );
        is_new
    }

    /// Убирает цель из бан-листа. `true` когда цель была в списке.
    pub fn unban(&mut self, target: &str) -> bool {
        self.banned.remove(target).is_some()
    }

    /// Все записи бан-листа, отсортированные по цели (для детерминированного вывода).
    pub fn entries(&self) -> Vec<&BanEntry> {
        let mut entries: Vec<&BanEntry> = self.banned.values().collect();
        entries.sort_by(|a, b| a.target.cmp(&b.target));
        entries
    }

    pub fn len(&self) -> usize {
        self.banned.len()
    }

    pub fn is_empty(&self) -> bool {
        self.banned.is_empty()
    }
}

#[cfg(test)]
mod banlist_tests {
    use super::*;

    fn temp_path(name: &str) -> std::path::PathBuf {
        let base = std::env::temp_dir().join(format!(
            "rfs_server_banlist_tests_{}_{}",
            name,
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock is before epoch")
                .as_nanos()
        ));
        std::fs::create_dir_all(&base).expect("temp dir is creatable");
        base.join("bans.json")
    }

    #[test]
    fn empty_list_is_not_banned() {
        let list = BanList::default();
        assert!(!list.is_banned("steam:123"));
        assert!(list.is_empty());
    }

    #[test]
    fn ban_and_unban() {
        let mut list = BanList::default();
        assert!(list.ban("steam:123".into(), "griefing".into(), "admin".into()));
        assert!(list.is_banned("steam:123"));
        // Re-ban is not new.
        assert!(!list.ban("steam:123".into(), "spam".into(), "admin2".into()));
        assert!(list.is_banned("steam:123"));
        assert_eq!(list.entries().len(), 1);
        assert!(list.unban("steam:123"));
        assert!(!list.is_banned("steam:123"));
        assert!(!list.unban("steam:123"));
        assert!(list.is_empty());
    }

    #[test]
    fn save_and_load_survives_restart() {
        let path = temp_path("persist");
        let mut list = BanList::default();
        list.ban("steam:456".into(), "cheating".into(), "console".into());
        list.save(&path).expect("the ban list saves");

        let loaded = BanList::load(&path).expect("the ban list loads");
        assert!(loaded.is_banned("steam:456"));
        assert_eq!(loaded.entries()[0].reason, "cheating");
        assert_eq!(loaded.entries()[0].banned_by, "console");
    }

    #[test]
    fn missing_file_creates_empty_list() {
        let path = temp_path("missing");
        assert!(!path.exists());
        let list = BanList::load(&path).expect("missing file creates empty list");
        assert!(path.exists(), "the first start must leave a ban list file");
        assert!(list.is_empty());
    }

    #[test]
    fn entries_are_sorted_by_target() {
        let mut list = BanList::default();
        list.ban("steam:b".into(), "r".into(), "a".into());
        list.ban("steam:a".into(), "r".into(), "a".into());
        list.ban("steam:c".into(), "r".into(), "a".into());
        let targets: Vec<String> = list
            .entries()
            .iter()
            .map(|e| e.target.clone())
            .collect();
        assert_eq!(targets, vec!["steam:a", "steam:b", "steam:c"]);
    }
}
