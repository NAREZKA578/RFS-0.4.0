use rfs_core::time::*;

// ---------- Tick::new / value ----------

#[test]
fn tick_new_and_value() {
    let t = Tick::new(42);
    assert_eq!(t.value(), 42);
}

#[test]
fn tick_default_is_zero() {
    let t = Tick::default();
    assert_eq!(t.value(), 0);
}

// ---------- Tick::next / prev ----------

#[test]
fn tick_next() {
    let t = Tick::new(10);
    assert_eq!(t.next().value(), 11);
}

#[test]
fn tick_prev() {
    let t = Tick::new(10);
    assert_eq!(t.prev().value(), 9);
}

#[test]
fn tick_prev_at_zero_no_underflow() {
    let t = Tick::new(0);
    assert_eq!(t.prev().value(), 0);
}

// ---------- Tick::duration_since ----------

#[test]
fn duration_since_self() {
    let t = Tick::new(50);
    assert_eq!(t.duration_since(Tick::new(50)).as_millis(), 0);
}

#[test]
fn duration_since_other_is_less() {
    let t = Tick::new(10);
    let d = t.duration_since(Tick::new(0));
    assert_eq!(d.as_millis(), 10 * TICK_DURATION_MS as u128);
}

#[test]
fn duration_since_other_greater_returns_zero() {
    let t = Tick::new(5);
    let d = t.duration_since(Tick::new(100));
    assert_eq!(d.as_millis(), 0);
}

// ---------- Tick arithmetic ----------

#[test]
fn tick_add_u64() {
    let t = Tick::new(10);
    let r = t + 5u64;
    assert_eq!(r.value(), 15);
}

#[test]
fn tick_sub_u64() {
    let t = Tick::new(10);
    let r = t - 3u64;
    assert_eq!(r.value(), 7);
}

#[test]
fn tick_sub_u64_saturates() {
    let t = Tick::new(3);
    let r = t - 10u64;
    assert_eq!(r.value(), 0);
}

#[test]
fn tick_sub_tick() {
    let a = Tick::new(20);
    let b = Tick::new(5);
    let r: u64 = a - b;
    assert_eq!(r, 15);
}

#[test]
fn tick_sub_tick_saturates() {
    let a = Tick::new(5);
    let b = Tick::new(20);
    let r: u64 = a - b;
    assert_eq!(r, 0);
}

// ---------- lerp_tick ----------

#[test]
fn lerp_tick_alpha_zero() {
    let r = lerp_tick(Tick::new(0), Tick::new(100), 0.0);
    assert_eq!(r.value(), 0);
}

#[test]
fn lerp_tick_alpha_one() {
    let r = lerp_tick(Tick::new(0), Tick::new(100), 1.0);
    assert_eq!(r.value(), 100);
}

#[test]
fn lerp_tick_alpha_half() {
    let r = lerp_tick(Tick::new(0), Tick::new(100), 0.5);
    assert_eq!(r.value(), 50);
}

#[test]
fn lerp_tick_same() {
    let r = lerp_tick(Tick::new(50), Tick::new(50), 0.7);
    assert_eq!(r.value(), 50);
}

// ---------- GameTime ----------

#[test]
fn game_time_new() {
    let gt = GameTime::new(Tick::new(10), 5.0, 0.1);
    assert_eq!(gt.tick.value(), 10);
    assert!((gt.time - 5.0).abs() < 0.001);
    assert!((gt.delta_time - 0.1).abs() < 0.001);
}

#[test]
fn game_time_from_tick_zero() {
    let gt = GameTime::from_tick(Tick::new(0));
    assert_eq!(gt.tick.value(), 0);
    assert!((gt.time - 0.0).abs() < 0.0001);
}

#[test]
fn game_time_from_tick_30() {
    let gt = GameTime::from_tick(Tick::new(30));
    assert!((gt.time - 1.0).abs() < 0.01);
}

// ---------- Constants sanity ----------

#[test]
fn tick_rate_is_30() {
    assert_eq!(TICK_RATE, 30);
}

#[test]
fn tick_duration_ms_is_33() {
    assert_eq!(TICK_DURATION_MS, 33);
}

#[test]
fn fixed_dt_approx() {
    assert!((FIXED_DT - 1.0 / 30.0).abs() < 0.0001);
}

// ---------- Tick equality / ordering ----------

#[test]
fn tick_ordering() {
    assert!(Tick::new(1) < Tick::new(2));
    assert!(Tick::new(5) > Tick::new(3));
    assert_eq!(Tick::new(7), Tick::new(7));
}
