use std::time::{Duration, Instant};
use serde::{Deserialize, Serialize};

pub const TICK_RATE: u32 = 30;
pub const TICK_DURATION_MS: u64 = 1000 / TICK_RATE as u64;
pub const TICK_DURATION: Duration = Duration::from_millis(TICK_DURATION_MS);
pub const FIXED_DT: f32 = 1.0 / TICK_RATE as f32;

#[derive(Copy, Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tick(pub u64);

impl Tick {
    pub fn new(value: u64) -> Self {
        Self(value)
    }

    pub fn next(&self) -> Self {
        Self(self.0 + 1)
    }

    pub fn prev(&self) -> Self {
        Self(self.0.saturating_sub(1))
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn duration_since(&self, other: Tick) -> Duration {
        Duration::from_millis((self.0.saturating_sub(other.0)) * TICK_DURATION_MS)
    }
}

impl std::ops::Add<u64> for Tick {
    type Output = Self;
    fn add(self, rhs: u64) -> Self {
        Self(self.0 + rhs)
    }
}

impl std::ops::Sub<u64> for Tick {
    type Output = Self;
    fn sub(self, rhs: u64) -> Self {
        Self(self.0.saturating_sub(rhs))
    }
}

impl std::ops::Sub<Tick> for Tick {
    type Output = u64;
    fn sub(self, rhs: Tick) -> u64 {
        self.0.saturating_sub(rhs.0)
    }
}

#[derive(Debug)]
pub struct TickTimer {
    last_tick: Instant,
    current_tick: Tick,
    accumulator: Duration,
}

impl TickTimer {
    pub fn new() -> Self {
        Self {
            last_tick: Instant::now(),
            current_tick: Tick(0),
            accumulator: Duration::ZERO,
        }
    }

    pub fn current_tick(&self) -> Tick {
        self.current_tick
    }

    pub fn tick(&mut self) -> bool {
        let now = Instant::now();
        let elapsed = now - self.last_tick;
        self.last_tick = now;
        self.accumulator += elapsed;

        if self.accumulator >= TICK_DURATION {
            self.accumulator -= TICK_DURATION;
            self.current_tick = self.current_tick.next();
            true
        } else {
            false
        }
    }

    pub fn should_tick(&self) -> bool {
        self.accumulator >= TICK_DURATION
    }

    pub fn alpha(&self) -> f32 {
        (self.accumulator.as_secs_f32() / TICK_DURATION.as_secs_f32()).clamp(0.0, 1.0)
    }

    pub fn sleep_until_next_tick(&self) {
        let remaining = TICK_DURATION.saturating_sub(self.accumulator);
        if !remaining.is_zero() {
            std::thread::sleep(remaining);
        }
    }

    pub fn reset(&mut self) {
        self.last_tick = Instant::now();
        self.current_tick = Tick(0);
        self.accumulator = Duration::ZERO;
    }
}

#[derive(Copy, Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GameTime {
    pub tick: Tick,
    pub time: f64,
    pub delta_time: f32,
}

impl GameTime {
    pub fn new(tick: Tick, time: f64, delta_time: f32) -> Self {
        Self { tick, time, delta_time }
    }

    pub fn from_tick(tick: Tick) -> Self {
        Self {
            tick,
            time: tick.value() as f64 * FIXED_DT as f64,
            delta_time: FIXED_DT,
        }
    }
}

#[derive(Copy, Clone, Debug)]
pub struct FrameTimer {
    last_frame: Instant,
    delta_time: f32,
    frame_count: u64,
    fps_timer: Instant,
    fps: f32,
}

impl FrameTimer {
    pub fn new() -> Self {
        Self {
            last_frame: Instant::now(),
            delta_time: 0.0,
            frame_count: 0,
            fps_timer: Instant::now(),
            fps: 0.0,
        }
    }

    pub fn tick(&mut self) -> f32 {
        let now = Instant::now();
        self.delta_time = (now - self.last_frame).as_secs_f32();
        self.last_frame = now;
        self.frame_count += 1;

        if self.fps_timer.elapsed().as_secs_f32() >= 1.0 {
            self.fps = self.frame_count as f32;
            self.frame_count = 0;
            self.fps_timer = Instant::now();
        }

        self.delta_time
    }

    pub fn delta_time(&self) -> f32 {
        self.delta_time
    }

    pub fn fps(&self) -> f32 {
        self.fps
    }
}

pub fn lerp_tick(a: Tick, b: Tick, alpha: f32) -> Tick {
    let a_val = a.value() as f32;
    let b_val = b.value() as f32;
    let result = a_val + (b_val - a_val) * alpha;
    Tick(result.round() as u64)
}