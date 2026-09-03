//! Фрустум-куллинг на `glam::Vec4`.
//!
//! Донор: `RFS-0.3/src/graphics/frustum.rs` (там уже убран собственный `Vec4`
//! в пользу `glam` — см. FRUSTUM-VEC4-1; сюда перенесён итог).

use glam::{Mat4, Vec4};

/// Шесть плоскостей в виде (normal.xyz, distance).
#[derive(Debug, Clone)]
pub struct Frustum {
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_view_proj(vp: &Mat4) -> Self {
        let c = vp.to_cols_array();
        let row = |i: usize| Vec4::new(c[i], c[i + 4], c[i + 8], c[i + 12]);
        let (r0, r1, r2, r3) = (row(0), row(1), row(2), row(3));
        let mut planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r3 + r2, r3 - r2];
        for p in &mut planes {
            let len = p.truncate().length().max(f32::EPSILON);
            *p /= len;
        }
        Self { planes }
    }

    /// Axis-aligned бокс; rotation модели должен запекаться вызывающим в min/max
    /// (урок FRUSTUM-1 из донора: culling без учёта rotation даёт ложные отсечения).
    pub fn test_aabb(&self, min: [f32; 3], max: [f32; 3]) -> bool {
        for p in &self.planes {
            let px = if p.x >= 0.0 { max[0] } else { min[0] };
            let py = if p.y >= 0.0 { max[1] } else { min[1] };
            let pz = if p.z >= 0.0 { max[2] } else { min[2] };
            if p.x * px + p.y * py + p.z * pz + p.w < 0.0 {
                return false;
            }
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::Camera;
    use glam::Vec3;

    #[test]
    fn origin_box_visible_from_default_camera() {
        let c = Camera::new(Vec3::new(0.0, 1.7, 5.0), 16.0 / 9.0);
        let f = Frustum::from_view_proj(&c.view_proj());
        assert!(f.test_aabb([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]));
    }

    #[test]
    fn box_behind_camera_culled() {
        let c = Camera::new(Vec3::new(0.0, 1.7, 5.0), 16.0 / 9.0);
        let f = Frustum::from_view_proj(&c.view_proj());
        assert!(!f.test_aabb([-1.0, -1.0, 10.0], [1.0, 1.0, 12.0]));
    }
}
