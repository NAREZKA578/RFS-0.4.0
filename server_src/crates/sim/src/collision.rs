use rfs_core::math::{Vec3f, Bounds, Transform};
use rfs_core::entity::EntityId;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionConfig {
    pub max_collision_depth: f32,
    pub restitution: f32,
    pub friction: f32,
    pub max_penetration_correction: f32,
    pub collision_margin: f32,
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            max_collision_depth: 10.0,
            restitution: 0.3,
            friction: 0.6,
            max_penetration_correction: 1.0,
            collision_margin: 0.05,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionShape {
    pub shape_type: CollisionShapeType,
    pub half_extents: Vec3f,
    pub radius: f32,
    pub height: f32,
    pub local_transform: Transform,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollisionShapeType {
    Box = 0,
    Sphere = 1,
    Capsule = 2,
    Cylinder = 3,
    ConvexHull = 4,
    TriangleMesh = 5,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionObject {
    pub entity_id: EntityId,
    pub shapes: Vec<CollisionShape>,
    pub transform: Transform,
    pub velocity: Vec3f,
    pub angular_velocity: Vec3f,
    pub mass: f32,
    pub is_static: bool,
    pub collision_layers: u32,
    pub collision_mask: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactPoint {
    pub point_a: Vec3f,
    pub point_b: Vec3f,
    pub normal: Vec3f,
    pub penetration: f32,
    pub shape_a: usize,
    pub shape_b: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionResult {
    pub object_a: EntityId,
    pub object_b: EntityId,
    pub contacts: SmallVec<[ContactPoint; 4]>,
    pub normal: Vec3f,
    pub penetration: f32,
}

pub struct CollisionSystem {
    config: CollisionConfig,
    objects: Vec<CollisionObject>,
    broadphase: Broadphase,
    contacts: Vec<CollisionResult>,
}

impl CollisionSystem {
    pub fn new(config: CollisionConfig) -> Self {
        Self {
            config,
            objects: Vec::new(),
            broadphase: Broadphase::new(),
            contacts: Vec::new(),
        }
    }

    pub fn add_object(&mut self, object: CollisionObject) {
        let id = object.entity_id;
        self.objects.push(object);
        self.broadphase.add_object(id);
    }

    pub fn remove_object(&mut self, entity_id: EntityId) {
        self.objects.retain(|o| o.entity_id != entity_id);
        self.broadphase.remove_object(entity_id);
    }

    pub fn update_object(&mut self, entity_id: EntityId, transform: Transform, velocity: Vec3f, angular_velocity: Vec3f) {
        if let Some(obj) = self.objects.iter_mut().find(|o| o.entity_id == entity_id) {
            obj.transform = transform;
            obj.velocity = velocity;
            obj.angular_velocity = angular_velocity;
            self.broadphase.update_object(entity_id, transform);
        }
    }

    pub fn step(&mut self) -> Vec<CollisionResult> {
        self.contacts.clear();
        
        let pairs = self.broadphase.get_pairs(&self.objects);
        
        for (idx_a, idx_b) in pairs {
            let obj_a = &self.objects[idx_a];
            let obj_b = &self.objects[idx_b];
            
            if !self.should_collide(obj_a, obj_b) {
                continue;
            }
            
            if let Some(result) = self.narrow_phase(obj_a, obj_b) {
                self.contacts.push(result);
            }
        }
        
        self.resolve_contacts();
        self.contacts.clone()
    }

    fn should_collide(&self, a: &CollisionObject, b: &CollisionObject) -> bool {
        (a.collision_layers & b.collision_mask) != 0 && (b.collision_layers & a.collision_mask) != 0
    }

    fn narrow_phase(&self, a: &CollisionObject, b: &CollisionObject) -> Option<CollisionResult> {
        let mut contacts = SmallVec::new();
        let mut max_penetration: f32 = 0.0;
        let mut avg_normal = Vec3f::ZERO;
        
        for (i, shape_a) in a.shapes.iter().enumerate() {
            for (j, shape_b) in b.shapes.iter().enumerate() {
                if let Some(contact) = self.shape_vs_shape(a, shape_a, i, b, shape_b, j) {
                    max_penetration = max_penetration.max(contact.penetration);
                    avg_normal += contact.normal;
                    contacts.push(contact);
                }
            }
        }
        
        if contacts.is_empty() {
            return None;
        }
        
        avg_normal = avg_normal.normalize();
        
        Some(CollisionResult {
            object_a: a.entity_id,
            object_b: b.entity_id,
            contacts,
            normal: avg_normal,
            penetration: max_penetration,
        })
    }

    fn shape_vs_shape(
        &self,
        a: &CollisionObject,
        shape_a: &CollisionShape,
        idx_a: usize,
        b: &CollisionObject,
        shape_b: &CollisionShape,
        idx_b: usize,
    ) -> Option<ContactPoint> {
        let world_a = a.transform * shape_a.local_transform;
        let world_b = b.transform * shape_b.local_transform;
        
        match (shape_a.shape_type, shape_b.shape_type) {
            (CollisionShapeType::Box, CollisionShapeType::Box) => {
                self.box_vs_box(world_a, shape_a.half_extents, world_b, shape_b.half_extents, idx_a, idx_b)
            }
            (CollisionShapeType::Sphere, CollisionShapeType::Sphere) => {
                self.sphere_vs_sphere(world_a, shape_a.radius, world_b, shape_b.radius, idx_a, idx_b)
            }
            (CollisionShapeType::Box, CollisionShapeType::Sphere) => {
                self.box_vs_sphere(world_a, shape_a.half_extents, world_b, shape_b.radius, idx_a, idx_b)
            }
            (CollisionShapeType::Sphere, CollisionShapeType::Box) => {
                self.box_vs_sphere(world_b, shape_b.half_extents, world_a, shape_a.radius, idx_b, idx_a)
                    .map(|mut c| { c.normal = -c.normal; std::mem::swap(&mut c.point_a, &mut c.point_b); c })
            }
            (CollisionShapeType::Capsule, CollisionShapeType::Capsule) => {
                self.capsule_vs_capsule(world_a, shape_a.radius, shape_a.height, world_b, shape_b.radius, shape_b.height, idx_a, idx_b)
            }
            _ => None,
        }
    }

    fn box_vs_box(
        &self,
        a: Transform,
        half_a: Vec3f,
        b: Transform,
        half_b: Vec3f,
        idx_a: usize,
        idx_b: usize,
    ) -> Option<ContactPoint> {
        let center_a = a.position;
        let center_b = b.position;
        let delta = center_b - center_a;
        
        let axes_a = [a.rotation.mul_vec3(Vec3f::RIGHT), a.rotation.mul_vec3(Vec3f::UP), a.rotation.mul_vec3(Vec3f::FORWARD)];
        let axes_b = [b.rotation.mul_vec3(Vec3f::RIGHT), b.rotation.mul_vec3(Vec3f::UP), b.rotation.mul_vec3(Vec3f::FORWARD)];
        
        let mut min_penetration = f32::MAX;
        let mut best_normal = Vec3f::ZERO;
        let best_point_a;
        let best_point_b;
        
        let all_axes = [
            axes_a[0], axes_a[1], axes_a[2],
            axes_b[0], axes_b[1], axes_b[2],
            axes_a[0].cross(axes_b[0]), axes_a[0].cross(axes_b[1]), axes_a[0].cross(axes_b[2]),
            axes_a[1].cross(axes_b[0]), axes_a[1].cross(axes_b[1]), axes_a[1].cross(axes_b[2]),
            axes_a[2].cross(axes_b[0]), axes_a[2].cross(axes_b[1]), axes_a[2].cross(axes_b[2]),
        ];
        
        for axis in all_axes {
            if axis.length_squared() < 0.0001 {
                continue;
            }
            let axis = axis.normalize();
            
            let proj_a = self.project_box(axis, half_a, axes_a);
            let proj_b = self.project_box(axis, half_b, axes_b);
            let center_dist = delta.dot(axis);
            
            let min_a = -proj_a;
            let max_a = proj_a;
            let min_b = center_dist - proj_b;
            let max_b = center_dist + proj_b;
            
            if max_a < min_b || max_b < min_a {
                return None;
            }
            
            let penetration = (max_a - min_b).min(max_b - min_a);
            if penetration < min_penetration {
                min_penetration = penetration;
                best_normal = if center_dist > 0.0 { axis } else { -axis };
            }
        }
        
        if min_penetration == f32::MAX || min_penetration > self.config.max_collision_depth {
            return None;
        }
        
        best_point_a = center_a + best_normal * (half_a.length() - min_penetration * 0.5);
        best_point_b = center_b - best_normal * (half_b.length() - min_penetration * 0.5);
        
        Some(ContactPoint {
            point_a: best_point_a,
            point_b: best_point_b,
            normal: best_normal,
            penetration: min_penetration,
            shape_a: idx_a,
            shape_b: idx_b,
        })
    }

    fn project_box(&self, axis: Vec3f, half: Vec3f, axes: [Vec3f; 3]) -> f32 {
        half.x * axis.dot(axes[0]).abs() +
        half.y * axis.dot(axes[1]).abs() +
        half.z * axis.dot(axes[2]).abs()
    }

    fn sphere_vs_sphere(
        &self,
        a: Transform,
        radius_a: f32,
        b: Transform,
        radius_b: f32,
        idx_a: usize,
        idx_b: usize,
    ) -> Option<ContactPoint> {
        let delta = b.position - a.position;
        let dist_sq = delta.length_squared();
        let radius_sum = radius_a + radius_b;
        
        if dist_sq > radius_sum * radius_sum {
            return None;
        }
        
        let dist = dist_sq.sqrt();
        let penetration = radius_sum - dist;
        
        if penetration > self.config.max_collision_depth {
            return None;
        }
        
        let normal = if dist > 0.001 { delta / dist } else { Vec3f::UP };
        let point_a = a.position + normal * radius_a;
        let point_b = b.position - normal * radius_b;
        
        Some(ContactPoint {
            point_a,
            point_b,
            normal,
            penetration,
            shape_a: idx_a,
            shape_b: idx_b,
        })
    }

    fn box_vs_sphere(
        &self,
        box_t: Transform,
        half: Vec3f,
        sphere_t: Transform,
        radius: f32,
        idx_a: usize,
        idx_b: usize,
    ) -> Option<ContactPoint> {
        let local_sphere = box_t.inverse().transform_point(sphere_t.position);
        
        let closest = Vec3f::new(
            local_sphere.x.clamp(-half.x, half.x),
            local_sphere.y.clamp(-half.y, half.y),
            local_sphere.z.clamp(-half.z, half.z),
        );
        
        let delta = local_sphere - closest;
        let dist_sq = delta.length_squared();
        
        if dist_sq > radius * radius {
            return None;
        }
        
        let dist = dist_sq.sqrt();
        let penetration = radius - dist;
        
        if penetration > self.config.max_collision_depth {
            return None;
        }
        
        let normal_local = if dist > 0.001 { delta / dist } else { Vec3f::UP };
        let normal = box_t.rotation.mul_vec3(normal_local);
        let point_a = box_t.transform_point(closest);
        let point_b = sphere_t.position - normal * radius;
        
        Some(ContactPoint {
            point_a,
            point_b,
            normal,
            penetration,
            shape_a: idx_a,
            shape_b: idx_b,
        })
    }

    fn capsule_vs_capsule(
        &self,
        a: Transform,
        radius_a: f32,
        height_a: f32,
        b: Transform,
        radius_b: f32,
        height_b: f32,
        idx_a: usize,
        idx_b: usize,
    ) -> Option<ContactPoint> {
        let half_height_a = height_a * 0.5;
        let half_height_b = height_b * 0.5;
        
        let a_top = a.transform_point(Vec3f::new(0.0, half_height_a, 0.0));
        let a_bottom = a.transform_point(Vec3f::new(0.0, -half_height_a, 0.0));
        let b_top = b.transform_point(Vec3f::new(0.0, half_height_b, 0.0));
        let b_bottom = b.transform_point(Vec3f::new(0.0, -half_height_b, 0.0));
        
        let (closest_a, closest_b) = self.closest_points_segment_segment(a_bottom, a_top, b_bottom, b_top);
        
        let delta = closest_b - closest_a;
        let dist_sq = delta.length_squared();
        let radius_sum = radius_a + radius_b;
        
        if dist_sq > radius_sum * radius_sum {
            return None;
        }
        
        let dist = dist_sq.sqrt();
        let penetration = radius_sum - dist;
        
        if penetration > self.config.max_collision_depth {
            return None;
        }
        
        let normal = if dist > 0.001 { delta / dist } else { Vec3f::UP };
        let point_a = closest_a + normal * radius_a;
        let point_b = closest_b - normal * radius_b;
        
        Some(ContactPoint {
            point_a,
            point_b,
            normal,
            penetration,
            shape_a: idx_a,
            shape_b: idx_b,
        })
    }

    fn closest_points_segment_segment(
        &self,
        p1: Vec3f, q1: Vec3f,
        p2: Vec3f, q2: Vec3f,
    ) -> (Vec3f, Vec3f) {
        let d1 = q1 - p1;
        let d2 = q2 - p2;
        let r = p1 - p2;
        let a = d1.dot(d1);
        let e = d2.dot(d2);
        let f = d2.dot(r);
        
        if a <= 0.0001 && e <= 0.0001 {
            return (p1, p2);
        }
        
        if a <= 0.0001 {
            let t = (f / e).clamp(0.0, 1.0);
            return (p1, p2 + d2 * t);
        }
        
        let c = d1.dot(r);
        if e <= 0.0001 {
            let t = (-c / a).clamp(0.0, 1.0);
            return (p1 + d1 * t, p2);
        }
        
        let b = d1.dot(d2);
        let denom = a * e - b * b;
        
        if denom != 0.0 {
            let t = (b * f - c * e) / denom;
            let t = t.clamp(0.0, 1.0);
            let u = (b * t + f) / e;
            let u = u.clamp(0.0, 1.0);
            return (p1 + d1 * t, p2 + d2 * u);
        }
        
        (p1, p2)
    }

    fn resolve_contacts(&mut self) {
        for contact in &self.contacts {
            let idx_a = self.objects.iter().position(|o| o.entity_id == contact.object_a);
            let idx_b = self.objects.iter().position(|o| o.entity_id == contact.object_b);
            
            if let (Some(ia), Some(ib)) = (idx_a, idx_b) {
                let (obj_a, obj_b) = if ia < ib {
                    let (left, right) = self.objects.split_at_mut(ib);
                    (&mut left[ia], &mut right[0])
                } else {
                    let (left, right) = self.objects.split_at_mut(ia);
                    (&mut right[0], &mut left[ib])
                };
                
                Self::resolve_contact(obj_a, obj_b, contact, self.config.restitution, self.config.max_penetration_correction);
            }
        }
    }

    fn resolve_contact(
        a: &mut CollisionObject,
        b: &mut CollisionObject,
        contact: &CollisionResult,
        restitution_config: f32,
        max_pen_corr: f32,
    ) {
        for cp in &contact.contacts {
            let normal = cp.normal;
            let penetration = cp.penetration;
            
            let r_a = cp.point_a - a.transform.position;
            let r_b = cp.point_b - b.transform.position;
            
            let v_a = a.velocity + a.angular_velocity.cross(r_a);
            let v_b = b.velocity + b.angular_velocity.cross(r_b);
            let rel_vel = v_b - v_a;
            let vel_along_normal = rel_vel.dot(normal);
            
            if vel_along_normal > 0.0 {
                continue;
            }
            
            let restitution = restitution_config.min(1.0);
            let inv_mass_a = if a.is_static { 0.0 } else { 1.0 / a.mass };
            let inv_mass_b = if b.is_static { 0.0 } else { 1.0 / b.mass };
            
            let mut impulse_mag = -(1.0 + restitution) * vel_along_normal;
            impulse_mag /= inv_mass_a + inv_mass_b;
            
            let max_impulse = penetration * 1000.0;
            impulse_mag = impulse_mag.clamp(-max_impulse, max_impulse);
            
            let impulse = normal * impulse_mag;
            
            if !a.is_static {
                a.velocity -= impulse * inv_mass_a;
                a.angular_velocity -= r_a.cross(impulse) * inv_mass_a;
            }
            
            if !b.is_static {
                b.velocity += impulse * inv_mass_b;
                b.angular_velocity += r_b.cross(impulse) * inv_mass_b;
            }
            
            let correction = normal * (penetration * 0.5 * max_pen_corr);
            if !a.is_static {
                a.transform.position -= correction;
            }
            if !b.is_static {
                b.transform.position += correction;
            }
        }
    }

    pub fn raycast(&self, origin: Vec3f, direction: Vec3f, max_distance: f32, mask: u32) -> Option<RaycastHit> {
        let mut closest_hit: Option<RaycastHit> = None;
        let mut closest_dist = max_distance;
        
        for obj in &self.objects {
            if (obj.collision_layers & mask) == 0 {
                continue;
            }
            
            for (i, shape) in obj.shapes.iter().enumerate() {
                let world_shape = obj.transform * shape.local_transform;
                
                if let Some(hit) = self.raycast_shape(origin, direction, &world_shape, shape, obj.entity_id, i) {
                    if hit.distance < closest_dist {
                        closest_dist = hit.distance;
                        closest_hit = Some(hit);
                    }
                }
            }
        }
        
        closest_hit
    }

    fn raycast_shape(
        &self,
        origin: Vec3f,
        direction: Vec3f,
        transform: &Transform,
        shape: &CollisionShape,
        entity_id: EntityId,
        shape_index: usize,
    ) -> Option<RaycastHit> {
        let local_origin = transform.inverse().transform_point(origin);
        let local_dir = transform.inverse().transform_vector(direction).normalize();
        
        match shape.shape_type {
            CollisionShapeType::Box => self.raycast_box(local_origin, local_dir, shape.half_extents),
            CollisionShapeType::Sphere => self.raycast_sphere(local_origin, local_dir, shape.radius),
            _ => None,
        }.map(|(dist, normal)| RaycastHit {
            entity_id,
            shape_index,
            distance: dist,
            point: origin + direction * dist,
            normal: transform.rotation.mul_vec3(normal),
        })
    }

    fn raycast_box(&self, origin: Vec3f, dir: Vec3f, half: Vec3f) -> Option<(f32, Vec3f)> {
        let inv_dir = Vec3f::new(1.0 / dir.x, 1.0 / dir.y, 1.0 / dir.z);
        let t1 = (-half - origin) * inv_dir;
        let t2 = (half - origin) * inv_dir;
        
        let tmin = Vec3f::new(t1.x.min(t2.x), t1.y.min(t2.y), t1.z.min(t2.z));
        let tmax = Vec3f::new(t1.x.max(t2.x), t1.y.max(t2.y), t1.z.max(t2.z));
        
        let enter = tmin.x.max(tmin.y).max(tmin.z);
        let exit = tmax.x.min(tmax.y).min(tmax.z);
        
        if exit < 0.0 || enter > exit {
            return None;
        }
        
        let dist = enter.max(0.0);
        let hit_point = origin + dir * dist;
        
        let mut normal = Vec3f::ZERO;
        let eps = 0.001;
        if (hit_point.x - half.x).abs() < eps { normal.x = 1.0; }
        else if (hit_point.x + half.x).abs() < eps { normal.x = -1.0; }
        else if (hit_point.y - half.y).abs() < eps { normal.y = 1.0; }
        else if (hit_point.y + half.y).abs() < eps { normal.y = -1.0; }
        else if (hit_point.z - half.z).abs() < eps { normal.z = 1.0; }
        else if (hit_point.z + half.z).abs() < eps { normal.z = -1.0; }
        
        Some((dist, normal.normalize()))
    }

    fn raycast_sphere(&self, origin: Vec3f, dir: Vec3f, radius: f32) -> Option<(f32, Vec3f)> {
        let oc = origin;
        let a = dir.dot(dir);
        let b = 2.0 * oc.dot(dir);
        let c = oc.dot(oc) - radius * radius;
        let disc = b * b - 4.0 * a * c;
        
        if disc < 0.0 {
            return None;
        }
        
        let sqrt_disc = disc.sqrt();
        let t = (-b - sqrt_disc) / (2.0 * a);
        
        if t < 0.0 {
            return None;
        }
        
        let hit_point = origin + dir * t;
        let normal = hit_point.normalize();
        
        Some((t, normal))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaycastHit {
    pub entity_id: EntityId,
    pub shape_index: usize,
    pub distance: f32,
    pub point: Vec3f,
    pub normal: Vec3f,
}

struct Broadphase {
    grid: HashMap<(i32, i32, i32), Vec<EntityId>>,
    cell_size: f32,
    object_bounds: HashMap<EntityId, Bounds>,
}

impl Broadphase {
    fn new() -> Self {
        Self {
            grid: HashMap::new(),
            cell_size: 100.0,
            object_bounds: HashMap::new(),
        }
    }

    fn add_object(&mut self, entity_id: EntityId) {
        self.object_bounds.insert(entity_id, Bounds::new(Vec3f::ZERO, Vec3f::ZERO));
    }

    fn remove_object(&mut self, entity_id: EntityId) {
        self.object_bounds.remove(&entity_id);
        for cell in self.grid.values_mut() {
            cell.retain(|&id| id != entity_id);
        }
    }

    fn update_object(&mut self, entity_id: EntityId, transform: Transform) {
        let bounds = Bounds::new(
            transform.position - Vec3f::new(50.0, 50.0, 50.0),
            transform.position + Vec3f::new(50.0, 50.0, 50.0),
        );
        
        if let Some(old_bounds) = self.object_bounds.get(&entity_id) {
            let old_min = self.world_to_cell(old_bounds.min);
            let old_max = self.world_to_cell(old_bounds.max);
            let new_min = self.world_to_cell(bounds.min);
            let new_max = self.world_to_cell(bounds.max);
            
            for x in old_min.0..=old_max.0 {
                for y in old_min.1..=old_max.1 {
                    for z in old_min.2..=old_max.2 {
                        let key = (x, y, z);
                        if x < new_min.0 || x > new_max.0 || y < new_min.1 || y > new_max.1 || z < new_min.2 || z > new_max.2 {
                            if let Some(cell) = self.grid.get_mut(&key) {
                                cell.retain(|&id| id != entity_id);
                            }
                        }
                    }
                }
            }
        }
        
        self.object_bounds.insert(entity_id, bounds);
        
        let min = self.world_to_cell(bounds.min);
        let max = self.world_to_cell(bounds.max);
        
        for x in min.0..=max.0 {
            for y in min.1..=max.1 {
                for z in min.2..=max.2 {
                    self.grid.entry((x, y, z)).or_default().push(entity_id);
                }
            }
        }
    }

    fn world_to_cell(&self, pos: Vec3f) -> (i32, i32, i32) {
        (
            (pos.x / self.cell_size).floor() as i32,
            (pos.y / self.cell_size).floor() as i32,
            (pos.z / self.cell_size).floor() as i32,
        )
    }

    fn get_pairs(&self, objects: &[CollisionObject]) -> Vec<(usize, usize)> {
        let mut pairs = Vec::new();
        let mut seen = std::collections::HashSet::new();
        
        for (i, obj_a) in objects.iter().enumerate() {
            let bounds = &self.object_bounds[&obj_a.entity_id];
            let min = self.world_to_cell(bounds.min);
            let max = self.world_to_cell(bounds.max);
            
            for x in min.0..=max.0 {
                for y in min.1..=max.1 {
                    for z in min.2..=max.2 {
                        if let Some(cell_objects) = self.grid.get(&(x, y, z)) {
                            for &entity_b in cell_objects {
                                if entity_b == obj_a.entity_id {
                                    continue;
                                }
                                
                                if let Some(j) = objects.iter().position(|o| o.entity_id == entity_b) {
                                    let key = if i < j { (i, j) } else { (j, i) };
                                    if seen.insert(key) {
                                        pairs.push(key);
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        
        pairs
    }
}