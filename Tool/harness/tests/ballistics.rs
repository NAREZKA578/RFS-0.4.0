// Moved out of crates/sim/src/ballistics.rs (side-only testing rule).
use rfs_core::math::Vec3f;
use rfs_core::packet::ProjectileType;
use rfs_sim::ballistics::{BallisticsCalculator, BallisticsConfig, ProjectileConfig};

#[test]
fn test_ballistic_arc() {
    let calc = BallisticsCalculator::new(BallisticsConfig::default());
    let from = Vec3f::new(0.0, 10.0, 0.0);
    let to = Vec3f::new(1000.0, 10.0, 0.0);

    let solution = calc.solve_ballistic_arc(from, to, ProjectileType::Cannonball);
    assert!(solution.is_some());

    let sol = solution.unwrap();
    assert!(sol.time_of_flight > 0.0);
    assert!(sol.pitch > 0.0);
}

#[test]
fn test_penetration() {
    let calc = BallisticsCalculator::new(BallisticsConfig::default());
    let config = ProjectileConfig::armor_piercing();

    let result = calc.calculate_penetration(
        &config,
        Vec3f::new(0.0, 0.0, -800.0),
        Vec3f::new(0.0, 0.0, 1.0),
        100.0,
        0.0,
    );

    assert!(result.penetrated);
    assert!(result.penetration_depth > 0.0);
}

#[test]
fn test_ricochet() {
    let calc = BallisticsCalculator::new(BallisticsConfig::default());
    let config = ProjectileConfig::cannonball();

    let result = calc.calculate_penetration(
        &config,
        Vec3f::new(0.0, -100.0, -400.0),
        Vec3f::new(0.0, 1.0, 0.0),
        50.0,
        1.3,
    );

    assert!(result.ricochet);
}
