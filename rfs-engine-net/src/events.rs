//! Очередь сетевых событий без тихого дропа.
//!
//! Донор: `RFS-0.3/src/net/handler.rs` (очереди `sound/particle/spawn/despawn`)
//! + дренаж в `src/app/update.rs:631-646`.
//!
//! Ошибка донора (№222 NET-EVENT-DROP-1): `pop_*` вызывались, но результат уходил
//! только в `tracing::debug!` — звук/партиклы/спавны фактически выбрасывались.
//! Здесь `drain` возвращает владение вызывающему; внутри — никакого логирования
//! и никакого дропа. Не обработал — это видно по типу, а не по логам.

#[derive(Debug, Clone, Default)]
pub struct EventQueue<T> {
    inner: Vec<T>,
}

impl<T> EventQueue<T> {
    pub fn push(&mut self, e: T) {
        self.inner.push(e);
    }

    pub fn len(&self) -> usize {
        self.inner.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// Забирает все накопленные события. Вызывающий обязан их обработать
    /// (звук → аудио, спавн → сцена). Пустой вызов — пустой `Vec`, без логов.
    pub fn drain(&mut self) -> Vec<T> {
        std::mem::take(&mut self.inner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drain_returns_ownership() {
        let mut q = EventQueue::default();
        q.push(1u32);
        q.push(2u32);
        let got = q.drain();
        assert_eq!(got, vec![1, 2]);
        assert!(q.is_empty());
    }
}
