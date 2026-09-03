//! Интерполяционный буфер, питаемый и снапшотами, и дельтами.
//!
//! Донор: `RFS-0.3/src/game/interpolation.rs` + подпитка в
//! `src/app/update.rs:660-691`.
//!
//! Ошибка донора (№220 INTERP-STARVE-1): `App.interpolation` пушился только
//! из `WorldSnapshot` (1/10 пакетов при FULL_UPDATE_INTERVAL=10), дельты
//! игнорировались — `interpolation_time` стоял 9/10 тиков.
//!
//! Здесь единый вход: `push_snapshot` для полных, `push_delta` для дельт.

use std::collections::{HashMap, VecDeque};

use glam::Vec3;

#[derive(Debug, Clone)]
pub struct Sample {
    pub pos: Vec3,
    pub time: f64,
}

#[derive(Debug, Clone, Default)]
pub struct InterpolationBuffer {
    tracks: HashMap<u32, VecDeque<Sample>>,
    pub interp_time: f64,
    pub render_delay: f64,
}

impl InterpolationBuffer {
    pub fn new(render_delay: f64) -> Self {
        Self {
            tracks: HashMap::new(),
            interp_time: 0.0,
            render_delay,
        }
    }

    fn push(&mut self, id: u32, pos: Vec3, time: f64) {
        let q = self.tracks.entry(id).or_insert_with(VecDeque::new);
        if q.len() >= 64 {
            q.pop_front();
        }
        q.push_back(Sample { pos, time });
        self.interp_time = time - self.render_delay;
    }

    /// Полный снапшот: все игроки.
    pub fn push_snapshot(&mut self, players: &[(u32, Vec3)], server_time: f64) {
        for (id, pos) in players {
            self.push(*id, *pos, server_time);
        }
    }

    /// FIX №220: дельта тоже двигает буфер.
    pub fn push_delta(&mut self, players: &[(u32, Vec3)], server_time: f64) {
        for (id, pos) in players {
            self.push(*id, *pos, server_time);
        }
    }

    /// Линейная интерполяция позиции на `interp_time`.
    pub fn sample(&self, id: u32) -> Option<Vec3> {
        let q = self.tracks.get(&id)?;
        if q.is_empty() || !self.interp_time.is_finite() {
            return None;
        }
        let mut prev = &q[0];
        for s in q.iter().skip(1) {
            if s.time >= self.interp_time {
                let span = (s.time - prev.time).max(f64::EPSILON);
                let t = ((self.interp_time - prev.time) / span).clamp(0.0, 1.0) as f32;
                return Some(prev.pos.lerp(s.pos, t));
            }
            prev = s;
        }
        Some(prev.pos)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delta_advances_interpolation_regression_220() {
        let mut b = InterpolationBuffer::new(0.1);
        b.push_snapshot(&[(1, Vec3::new(0.0, 0.0, 0.0))], 1.0);
        // Раньше этот вызов не существовал — 9/10 тиков буфер стоял.
        b.push_delta(&[(1, Vec3::new(10.0, 0.0, 0.0))], 1.1);
        assert!(b.interp_time > 0.9);
        assert!(b.sample(1).is_some());
    }
}
