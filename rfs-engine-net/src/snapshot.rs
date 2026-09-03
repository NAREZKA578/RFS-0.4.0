//! Снапшот и дельта с merge-семантикой.
//!
//! Донор: `RFS-0.3/src/net/handler.rs:234-321` + `snapshot.rs:442-457`.
//!
//! Ошибка донора (№219 DELTA-WIPE-1): клиент делал
//! `fire_sources.clone_from(updated_fires)`, а сервер 9/10 тиков слал
//! `updated_fires = []` → клиент стирал пожары до следующего полного снапшота.
//! Гидранты рядом были обработаны правильно (`if !empty`), что и взято за образец.
//!
//! Контракт этого модуля:
//! - `apply_snapshot` — полная замена (только для полных снапшотов).
//! - `apply_delta` — только merge: upsert по id + удаление по id-спискам.
//!   Пустые списки — no-op, а не wipe.

use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Id(pub u32);

#[derive(Debug, Clone, PartialEq)]
pub struct Fire {
    pub id: u32,
    pub pos: [f32; 3],
    pub intensity: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Civilian {
    pub id: u32,
    pub pos: [f32; 3],
}

#[derive(Debug, Clone, PartialEq)]
pub struct Hydrant {
    pub id: u32,
    pub pos: [f32; 3],
}

/// Полное состояние мира со стороны сети.
#[derive(Debug, Clone, Default)]
pub struct SnapshotView {
    pub fires: HashMap<u32, Fire>,
    pub civs: HashMap<u32, Civilian>,
    pub hydrants: HashMap<u32, Hydrant>,
}

/// Инкрементальное обновление. `None` у гидрантов = «без изменений»
/// (сервер шлёт их редко — см. FULL_UPDATE_INTERVAL в доноре).
#[derive(Debug, Clone, Default)]
pub struct Delta {
    pub upsert_fires: Vec<Fire>,
    pub extinguished: Vec<u32>,
    pub upsert_civs: Vec<Civilian>,
    pub rescued: Vec<u32>,
    pub upsert_hydrants: Option<Vec<Hydrant>>,
}

impl SnapshotView {
    pub fn apply_snapshot(
        &mut self,
        fires: Vec<Fire>,
        civs: Vec<Civilian>,
        hydrants: Vec<Hydrant>,
    ) {
        self.fires = fires.into_iter().map(|f| (f.id, f)).collect();
        self.civs = civs.into_iter().map(|c| (c.id, c)).collect();
        self.hydrants = hydrants.into_iter().map(|h| (h.id, h)).collect();
    }

    /// FIX №219: merge, никогда wipe. Пустой `upsert_*` ничего не трогает.
    pub fn apply_delta(&mut self, d: &Delta) {
        for f in &d.upsert_fires {
            self.fires.insert(f.id, f.clone());
        }
        for id in &d.extinguished {
            self.fires.remove(id);
        }
        for c in &d.upsert_civs {
            self.civs.insert(c.id, c.clone());
        }
        for id in &d.rescued {
            self.civs.remove(id);
        }
        if let Some(hs) = &d.upsert_hydrants {
            for h in hs {
                self.hydrants.insert(h.id, h.clone());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fire(id: u32) -> Fire {
        Fire {
            id,
            pos: [id as f32, 0.0, 0.0],
            intensity: 50.0,
        }
    }

    #[test]
    fn empty_delta_preserves_state_regression_219() {
        let mut v = SnapshotView::default();
        v.apply_snapshot(vec![fire(1), fire(2)], vec![], vec![]);
        // Дельта-тик донора: всё пусто. Раньше это стирало пожары.
        v.apply_delta(&Delta::default());
        assert_eq!(v.fires.len(), 2);
    }

    #[test]
    fn delta_merges_and_removes() {
        let mut v = SnapshotView::default();
        v.apply_snapshot(vec![fire(1), fire(2)], vec![], vec![]);
        v.apply_delta(&Delta {
            upsert_fires: vec![fire(3)],
            extinguished: vec![1],
            ..Default::default()
        });
        assert!(!v.fires.contains_key(&1));
        assert!(v.fires.contains_key(&2));
        assert!(v.fires.contains_key(&3));
    }
}
