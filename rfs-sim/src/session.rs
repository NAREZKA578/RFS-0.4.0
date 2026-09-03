//! Сессии, carry гражданских и гидранты — с единым путём удаления.
//!
//! Донор: reconnect-ветка `connection.rs:191-201` чистила только игрока
//! (`world.remove_player` + `health remove` + recycle ID), а полный
//! `disconnect_player` (`server/mod.rs:1479-1511`) дополнительно дропал
//! несомых (`drop_civilian` + broadcast `CivilianDrop`) и отключал гидранты.
//!
//! Ошибка донора (№221 RECONNECT-LEAK-1): т.к. ID тут же переиспользовался
//! (стек LIFO: push старого → pop тому же переподключению), новая сессия молча
//! наследовала `carried_by`/`connected_by`, а остальные клиенты не получали
//! `CivilianDrop`.
//!
//! Контракт: ЛЮБОЕ удаление сессии — только через `remove_session`, которая
//! возвращает `BroadcastEvents`. Вызывающий (сеть) обязан их разослать —
//! и для disconnect, и для reconnect с того же адреса.

use std::collections::{HashMap, HashSet};

pub type PlayerId = u32;
pub type CivilianId = u32;
pub type HydrantId = u32;

/// Что сеть обязана разослать после удаления сессии.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BroadcastEvents {
    /// Гражданские, которых нёс игрок: их carry сброшен, клиенты должны обновить.
    pub civilian_drops: Vec<CivilianId>,
    /// Гидранты, отключённые от игрока.
    pub hydrant_disconnects: Vec<HydrantId>,
    /// Освобождённый ID для переиспользования.
    pub recycled_id: Option<PlayerId>,
}

#[derive(Debug, Clone, Default)]
pub struct SessionStore {
    sessions: HashSet<PlayerId>,
    carried: HashMap<CivilianId, PlayerId>,
    hydrants: HashMap<HydrantId, PlayerId>,
    free_ids: Vec<PlayerId>,
    next_id: PlayerId,
}

impl SessionStore {
    pub fn new() -> Self {
        Self {
            next_id: 1,
            ..Default::default()
        }
    }

    pub fn connect(&mut self) -> PlayerId {
        let id = self.free_ids.pop().unwrap_or_else(|| {
            let id = self.next_id;
            self.next_id = self.next_id.saturating_add(1);
            id
        });
        self.sessions.insert(id);
        id
    }

    pub fn pick_up(&mut self, civ: CivilianId, player: PlayerId) -> bool {
        if !self.sessions.contains(&player) || self.carried.contains_key(&civ) {
            return false;
        }
        self.carried.insert(civ, player);
        true
    }

    pub fn connect_hydrant(&mut self, h: HydrantId, player: PlayerId) -> bool {
        if !self.sessions.contains(&player) {
            return false;
        }
        self.hydrants.insert(h, player);
        true
    }

    /// FIX №221: единый путь. Сбрасывает carry/гидранты, возвращает события
    /// для broadcast и освобождённый ID. Использовать и при disconnect,
    /// и при чистке старой сессии в reconnect-ветке (до `connections.remove`).
    pub fn remove_session(&mut self, player: PlayerId) -> BroadcastEvents {
        self.sessions.remove(&player);
        let mut ev = BroadcastEvents {
            recycled_id: Some(player),
            ..Default::default()
        };
        let carried: Vec<CivilianId> = self
            .carried
            .iter()
            .filter(|(_, p)| **p == player)
            .map(|(c, _)| *c)
            .collect();
        for c in carried {
            self.carried.remove(&c);
            ev.civilian_drops.push(c);
        }
        let owned: Vec<HydrantId> = self
            .hydrants
            .iter()
            .filter(|(_, p)| **p == player)
            .map(|(h, _)| *h)
            .collect();
        for h in owned {
            self.hydrants.remove(&h);
            ev.hydrant_disconnects.push(h);
        }
        self.free_ids.push(player);
        ev
    }

    pub fn carrier_of(&self, civ: CivilianId) -> Option<PlayerId> {
        self.carried.get(&civ).copied()
    }

    pub fn hydrant_owner(&self, h: HydrantId) -> Option<PlayerId> {
        self.hydrants.get(&h).copied()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reconnect_path_does_not_inherit_carry_regression_221() {
        let mut s = SessionStore::new();
        let old = s.connect();
        assert!(s.pick_up(7, old));
        assert!(s.connect_hydrant(9, old));
        // Чистка старой сессии (reconnect с того же адреса) — тем же путём.
        let ev = s.remove_session(old);
        assert_eq!(ev.civilian_drops, vec![7]);
        assert_eq!(ev.hydrant_disconnects, vec![9]);
        // Новая сессия (LIFO — тот же ID) уже ничего не несёт.
        let fresh = s.connect();
        assert_eq!(fresh, old);
        assert_eq!(s.carrier_of(7), None);
        assert_eq!(s.hydrant_owner(9), None);
    }

    #[test]
    fn double_carry_rejected() {
        let mut s = SessionStore::new();
        let a = s.connect();
        let b = s.connect();
        assert!(s.pick_up(1, a));
        assert!(!s.pick_up(1, b));
    }
}
