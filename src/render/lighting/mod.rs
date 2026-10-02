//! Lighting Module
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**

pub mod light;
pub mod probe;
pub mod shadows;

pub use light::{DirectionalLight, Light, LightConfig, LightType, PointLight, SpotLight};
pub use probe::{LightProbe, LightProbeConfig, LightProbeType};
pub use shadows::{CascadedShadowConfig, ShadowConfig};

#[cfg(test)]
mod tests {
    use super::probe::*;
    use super::shadows::{camera_basis, safe_up, CascadedShadowConfig, CascadedShadowMap};
    use glam::{Mat4, Vec3};

    /// Bug №183: a negative coordinate made `.floor() as usize` wrap to
    /// `usize::MAX`, so the whole negative half-space silently had no probes.
    #[test]
    fn a_negative_position_has_no_probe_instead_of_wrapping() {
        let grid = LightProbeGrid::new(
            LightProbeConfig::default(),
            Vec3::new(4.0, 4.0, 4.0),
            Vec3::new(10.0, 10.0, 10.0),
        );
        assert!(
            grid.get_probe(Vec3::new(-1.0, 0.0, 0.0)).is_none(),
            "a negative coordinate must not wrap into the grid"
        );
        assert!(grid.get_probe(Vec3::new(-1.0, -1.0, -1.0)).is_none());
        // And it must not panic either.
        let _ = grid.get_probe(Vec3::new(-1.0e9, 0.0, 0.0));
    }

    /// Bug №183: a position past the last cell must not alias a different one.
    #[test]
    fn a_position_past_the_grid_returns_none() {
        let grid = LightProbeGrid::new(
            LightProbeConfig::default(),
            Vec3::new(4.0, 4.0, 4.0),
            Vec3::new(10.0, 10.0, 10.0),
        );
        assert!(grid.get_probe(Vec3::new(1000.0, 0.0, 0.0)).is_none());
        assert!(grid.get_probe(Vec3::new(0.0, 0.0, 1000.0)).is_none());
        // In range works.
        assert!(grid.get_probe(Vec3::new(5.0, 5.0, 5.0)).is_some());
    }

    /// Bug №183: non-finite input and a degenerate cell size must be handled.
    #[test]
    fn non_finite_input_and_degenerate_grids_are_rejected() {
        let cfg = LightProbeConfig::default();
        assert!(LightProbeGrid::try_new(cfg.clone(), Vec3::new(2.0, 2.0, 2.0), Vec3::ZERO).is_none());
        assert!(LightProbeGrid::try_new(cfg.clone(), Vec3::ZERO, Vec3::new(1.0, 1.0, 1.0)).is_none());
        assert!(LightProbeGrid::try_new(cfg.clone(), Vec3::new(f32::NAN, 2.0, 2.0), Vec3::new(1.0, 1.0, 1.0)).is_none());
        // A valid grid is accepted and has the right probe count.
        let g = LightProbeGrid::try_new(cfg, Vec3::new(2.0, 3.0, 4.0), Vec3::new(1.0, 1.0, 1.0)).unwrap();
        assert_eq!(g.probes.len(), 24);

        let ok = LightProbeGrid::new(LightProbeConfig::default(), Vec3::new(2.0, 2.0, 2.0), Vec3::new(1.0, 1.0, 1.0));
        assert!(ok.get_probe(Vec3::new(f32::NAN, 0.0, 0.0)).is_none());
        assert!(ok.get_probe(Vec3::new(f32::INFINITY, 0.0, 0.0)).is_none());
    }

    /// Bug №184: a light direction parallel to the up-vector used to collapse
    /// the `look_at` basis to NaN, poisoning every cascade matrix.
    #[test]
    fn safe_up_is_never_parallel_to_the_light() {
        for dir in [
            Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::NEG_X, Vec3::Z, Vec3::NEG_Z,
            Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.3, -0.9, 0.1),
        ] {
            let d = dir.normalize();
            let up = safe_up(d);
            let cross = d.cross(up);
            assert!(
                cross.length() > 1.0e-3,
                "up {up:?} is parallel to direction {d:?} (cross = {cross:?})"
            );
        }
    }

    /// Bug №184: the cascade matrices must be finite for every light direction,
    /// including the degenerate ones.
    #[test]
    fn cascade_matrices_stay_finite_for_every_light_direction() {
        for dir in [Vec3::Y, Vec3::NEG_Y, Vec3::new(0.0, 1.0, 0.0), Vec3::X] {
            let mut csm = CascadedShadowMap::new(CascadedShadowConfig::default(), dir);
            let view = Mat4::look_at_rh(Vec3::new(5.0, 5.0, 5.0), Vec3::ZERO, Vec3::Y);
            let proj = Mat4::perspective_rh_gl(std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 500.0);
            csm.update(view, proj, &CascadedShadowConfig::default().cascade_distances);

            for i in 0..csm.config.cascade_count {
                let m = csm.get_cascade_matrix(i);
                assert!(
                    m.as_ref().iter().all(|v| v.is_finite()),
                    "cascade {i} is not finite for light {dir:?}: {m:?}"
                );
            }
        }
    }

    /// Bug №184: the cascades must follow the camera, not sit at the origin.
    #[test]
    fn cascades_follow_the_camera() {
        let cfg = CascadedShadowConfig::default();
        let proj = Mat4::perspective_rh_gl(std::f32::consts::FRAC_PI_4, 16.0 / 9.0, 0.1, 500.0);

        let mut at_origin = CascadedShadowMap::new(cfg.clone(), Vec3::new(0.3, -1.0, 0.2));
        at_origin.update(Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y), proj, &cfg.cascade_distances);

        // Move the camera far away and re-fit.
        let eye = Vec3::new(5000.0, 0.0, 5000.0);
        let mut moved = CascadedShadowMap::new(cfg.clone(), Vec3::new(0.3, -1.0, 0.2));
        moved.update(Mat4::look_at_rh(eye, eye + Vec3::NEG_Z, Vec3::Y), proj, &cfg.cascade_distances);

        let a = at_origin.get_cascade_matrix(0);
        let b = moved.get_cascade_matrix(0);
        assert_ne!(
            a.to_cols_array(),
            b.to_cols_array(),
            "the cascade did not move with the camera"
        );
    }

    /// Bug №184: a degenerate cascade distance must not produce a singular or
    /// non-finite matrix.
    #[test]
    fn degenerate_cascade_distances_are_handled() {
        let cfg = CascadedShadowConfig::default();
        let mut csm = CascadedShadowMap::new(cfg, Vec3::new(0.3, -1.0, 0.2));
        let proj = Mat4::perspective_rh_gl(std::f32::consts::FRAC_PI_4, 1.0, 0.1, 100.0);
        let view = Mat4::look_at_rh(Vec3::ZERO, Vec3::NEG_Z, Vec3::Y);
        // Unsorted, equal and non-finite distances.
        csm.update(view, proj, &[10.0, 10.0, f32::NAN, -5.0, 0.0]);
        for i in 0..csm.config.cascade_count {
            let m = csm.get_cascade_matrix(i);
            assert!(
                m.as_ref().iter().all(|v| v.is_finite()),
                "cascade {i} went non-finite from a bad distance list"
            );
        }
    }

    /// A camera_basis round-trip: the recovered position and forward must match
    /// what the view matrix encodes.
    #[test]
    fn camera_basis_recovers_position_and_forward() {
        let eye = Vec3::new(12.0, -4.0, 30.0);
        let target = Vec3::new(-3.0, 2.0, 0.0);
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        let (pos, forward) = camera_basis(view);
        assert!((pos - eye).length() < 1.0e-3, "position {pos:?} != {eye:?}");
        let expected = (target - eye).normalize();
        assert!(
            (forward - expected).length() < 1.0e-3,
            "forward {forward:?} != {expected:?}"
        );
    }

    /// The identity view encodes the camera at the origin looking down -Z.
    #[test]
    fn camera_basis_handles_the_identity_view() {
        let (pos, forward) = camera_basis(Mat4::IDENTITY);
        assert_eq!(pos, Vec3::ZERO);
        assert!((forward - Vec3::NEG_Z).length() < 1.0e-6);
    }
}
