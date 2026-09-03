//! Свет движка. На старте — только directional (как в доноре
//! `STANDARD_FS`); point-light от огня — запланированное расширение, под него
//! уже заложен `Vec<PointLight>` с явным лимитом, а не «добавим потом как-нибудь».

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Направленный свет (солнце).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DirectionalLight {
    pub direction: Vec3,
    pub color: Vec3,
    pub intensity: f32,
}

/// Точечный свет (огонь, фонари). Пока не подаётся в шейдер — структура
/// зафиксирована, чтобы сим и рендер сошлись на формате заранее.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PointLight {
    pub position: Vec3,
    pub color: Vec3,
    pub intensity: f32,
    pub radius: f32,
}

/// Сколько point-light тянет шейдерная модель движка v1.
pub const MAX_POINT_LIGHTS: usize = 16;

/// Набор света кадра.
#[derive(Debug, Clone, Default)]
pub struct LightingRig {
    pub sun: Option<DirectionalLight>,
    pub points: Vec<PointLight>,
}

impl LightingRig {
    pub fn push_point(&mut self, light: PointLight) {
        if self.points.len() < MAX_POINT_LIGHTS {
            self.points.push(light);
        }
    }
}
