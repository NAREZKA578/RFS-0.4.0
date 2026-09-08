use rfs_core::entity::EntityId;
use petgraph::graph::{Graph, NodeIndex, EdgeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentNode {
    pub entity_id: EntityId,
    pub name: String,
    pub max_capacity: f32,
    pub current_level: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub pump_capacity: f32,
    pub pump_active: bool,
    pub fire_intensity: f32,
    pub damage: f32,
    pub max_damage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BulkheadEdge {
    pub entity_id: EntityId,
    pub connects_a: EntityId,
    pub connects_b: EntityId,
    pub is_sealed: bool,
    pub is_destroyed: bool,
    pub seal_strength: f32,
    pub flow_resistance: f32,
    pub health: f32,
    pub max_health: f32,
}

impl CompartmentNode {
    pub fn new(entity_id: EntityId, name: String, max_capacity: f32, pump_capacity: f32) -> Self {
        Self {
            entity_id,
            name,
            max_capacity,
            current_level: 0.0,
            is_sealed: true,
            is_breached: false,
            pump_capacity,
            pump_active: false,
            fire_intensity: 0.0,
            damage: 0.0,
            max_damage: 1000.0,
        }
    }

    pub fn fill_ratio(&self) -> f32 {
        if self.max_capacity > 0.0 {
            self.current_level / self.max_capacity
        } else {
            0.0
        }
    }

    pub fn is_flooded(&self) -> bool {
        self.fill_ratio() > 0.9
    }

    pub fn is_critical(&self) -> bool {
        self.fill_ratio() > 0.5
    }

    pub fn can_pump(&self) -> bool {
        self.pump_active && self.current_level > 0.0 && !self.is_breached
    }
}

impl BulkheadEdge {
    pub fn new(entity_id: EntityId, connects_a: EntityId, connects_b: EntityId, seal_strength: f32) -> Self {
        Self {
            entity_id,
            connects_a,
            connects_b,
            is_sealed: true,
            is_destroyed: false,
            seal_strength,
            flow_resistance: 1.0 / seal_strength.max(0.1),
            health: seal_strength * 10.0,
            max_health: seal_strength * 10.0,
        }
    }

    pub fn is_passable(&self) -> bool {
        !self.is_sealed || self.is_destroyed
    }

    pub fn effective_resistance(&self) -> f32 {
        if self.is_passable() {
            0.1
        } else {
            self.flow_resistance
        }
    }
}

pub type CompartmentGraph = Graph<CompartmentNode, BulkheadEdge>;

pub struct DamageGraph {
    graph: CompartmentGraph,
    node_map: HashMap<EntityId, NodeIndex>,
    edge_map: HashMap<EntityId, EdgeIndex>,
}

impl DamageGraph {
    pub fn new() -> Self {
        Self {
            graph: Graph::new(),
            node_map: HashMap::new(),
            edge_map: HashMap::new(),
        }
    }

    pub fn add_compartment(&mut self, node: CompartmentNode) -> NodeIndex {
        let idx = self.graph.add_node(node.clone());
        self.node_map.insert(node.entity_id, idx);
        idx
    }

    pub fn remove_compartment(&mut self, entity_id: EntityId) -> Option<CompartmentNode> {
        if let Some(idx) = self.node_map.remove(&entity_id) {
            self.graph.remove_node(idx)
        } else {
            None
        }
    }

    pub fn add_bulkhead(&mut self, edge: BulkheadEdge) -> Option<EdgeIndex> {
        let idx_a = self.node_map.get(&edge.connects_a)?;
        let idx_b = self.node_map.get(&edge.connects_b)?;
        
        let edge_idx = self.graph.add_edge(*idx_a, *idx_b, edge.clone());
        self.edge_map.insert(edge.entity_id, edge_idx);
        Some(edge_idx)
    }

    pub fn remove_bulkhead(&mut self, entity_id: EntityId) -> Option<BulkheadEdge> {
        if let Some(idx) = self.edge_map.remove(&entity_id) {
            self.graph.remove_edge(idx)
        } else {
            None
        }
    }

    pub fn get_compartment(&self, entity_id: EntityId) -> Option<&CompartmentNode> {
        self.node_map.get(&entity_id).map(|idx| &self.graph[*idx])
    }

    pub fn get_compartment_mut(&mut self, entity_id: EntityId) -> Option<&mut CompartmentNode> {
        self.node_map.get(&entity_id).map(|idx| &mut self.graph[*idx])
    }

    pub fn get_bulkhead(&self, entity_id: EntityId) -> Option<&BulkheadEdge> {
        self.edge_map.get(&entity_id).map(|idx| &self.graph[*idx])
    }

    pub fn get_bulkhead_mut(&mut self, entity_id: EntityId) -> Option<&mut BulkheadEdge> {
        self.edge_map.get(&entity_id).map(|idx| &mut self.graph[*idx])
    }

    pub fn get_connected_compartments(&self, entity_id: EntityId) -> SmallVec<[EntityId; 4]> {
        let mut result = SmallVec::new();
        if let Some(idx) = self.node_map.get(&entity_id) {
            for edge in self.graph.edges(*idx) {
                let other = if edge.source() == *idx { edge.target() } else { edge.source() };
                result.push(self.graph[other].entity_id);
            }
        }
        result
    }

    pub fn get_bulkhead_between(&self, a: EntityId, b: EntityId) -> Option<EntityId> {
        if let (Some(idx_a), Some(idx_b)) = (self.node_map.get(&a), self.node_map.get(&b)) {
            for edge in self.graph.edges_connecting(*idx_a, *idx_b) {
                return Some(edge.weight().entity_id);
            }
        }
        None
    }

    pub fn seal_bulkhead(&mut self, bulkhead_id: EntityId) -> bool {
        if let Some(edge) = self.get_bulkhead_mut(bulkhead_id) {
            edge.is_sealed = true;
            true
        } else {
            false
        }
    }

    pub fn unseal_bulkhead(&mut self, bulkhead_id: EntityId) -> bool {
        if let Some(edge) = self.get_bulkhead_mut(bulkhead_id) {
            edge.is_sealed = false;
            true
        } else {
            false
        }
    }

    pub fn damage_bulkhead(&mut self, bulkhead_id: EntityId, damage: f32) -> bool {
        if let Some(edge) = self.get_bulkhead_mut(bulkhead_id) {
            edge.health -= damage;
            if edge.health <= 0.0 {
                edge.is_destroyed = true;
                edge.is_sealed = false;
                edge.flow_resistance = 0.1;
            }
            true
        } else {
            false
        }
    }

    pub fn repair_bulkhead(&mut self, bulkhead_id: EntityId, amount: f32) -> bool {
        if let Some(edge) = self.get_bulkhead_mut(bulkhead_id) {
            edge.health = (edge.health + amount).min(edge.max_health);
            if edge.health > 0.0 && edge.is_destroyed {
                edge.is_destroyed = false;
            }
            true
        } else {
            false
        }
    }

    pub fn breach_compartment(&mut self, compartment_id: EntityId) -> bool {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            node.is_breached = true;
            node.is_sealed = false;
            true
        } else {
            false
        }
    }

    pub fn seal_compartment(&mut self, compartment_id: EntityId) -> bool {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            node.is_sealed = true;
            true
        } else {
            false
        }
    }

    pub fn add_water(&mut self, compartment_id: EntityId, amount: f32) -> f32 {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            let space = node.max_capacity - node.current_level;
            let added = amount.min(space);
            node.current_level += added;
            added
        } else {
            0.0
        }
    }

    pub fn pump_water(&mut self, compartment_id: EntityId, amount: f32) -> f32 {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            let pumped = amount.min(node.current_level);
            node.current_level -= pumped;
            pumped
        } else {
            0.0
        }
    }

    pub fn set_fire(&mut self, compartment_id: EntityId, intensity: f32) -> bool {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            node.fire_intensity = intensity.clamp(0.0, 1.0);
            true
        } else {
            false
        }
    }

    pub fn extinguish_fire(&mut self, compartment_id: EntityId) -> bool {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            node.fire_intensity = 0.0;
            true
        } else {
            false
        }
    }

    pub fn apply_damage(&mut self, compartment_id: EntityId, damage: f32) -> bool {
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            node.damage = (node.damage + damage).min(node.max_damage);
            if node.damage >= node.max_damage {
                node.is_breached = true;
                node.is_sealed = false;
            }
            true
        } else {
            false
        }
    }

    pub fn compartment_count(&self) -> usize {
        self.graph.node_count()
    }

    pub fn bulkhead_count(&self) -> usize {
        self.graph.edge_count()
    }

    pub fn all_compartments(&self) -> Vec<&CompartmentNode> {
        self.graph.node_weights().collect()
    }

    pub fn all_bulkheads(&self) -> Vec<&BulkheadEdge> {
        self.graph.edge_weights().collect()
    }

    pub fn iter_nodes(&self) -> impl Iterator<Item = (EntityId, &CompartmentNode)> {
        self.graph.node_indices().map(|idx| (self.graph[idx].entity_id, &self.graph[idx]))
    }

    pub fn iter_edges(&self) -> impl Iterator<Item = (EntityId, &BulkheadEdge)> {
        self.graph.edge_references().map(|edge| (edge.weight().entity_id, edge.weight()))
    }
}