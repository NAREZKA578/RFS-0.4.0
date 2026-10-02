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
    /// Creates an intact bulkhead in the CLOSED (sealed) state.
    ///
    /// Bug №241: `damage.rs` builds synthetic bulkheads with this constructor
    /// specifically so a flooded compartment has somewhere to flow, and then
    /// comments that this is what lets the water propagate. They did not: the
    /// old body set `is_sealed: true`, and `is_passable()` is
    /// `!is_sealed || is_destroyed`, so every synthetic edge was permanently
    /// impassable and the water never moved. The fix belongs at the synthetic
    /// call site, which now uses `open` below.
    pub fn new(
        entity_id: EntityId,
        connects_a: EntityId,
        connects_b: EntityId,
        seal_strength: f32,
    ) -> Self {
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

    /// Creates a bulkhead in the OPEN state, ready to be closed explicitly.
    ///
    /// This is what the synthetic water-flow edges in `damage.rs` must use: a
    /// bulkhead is only a barrier if it is sealed, and a freshly wired
    /// structural edge is not.
    pub fn open(
        entity_id: EntityId,
        connects_a: EntityId,
        connects_b: EntityId,
        seal_strength: f32,
    ) -> Self {
        let mut edge = Self::new(entity_id, connects_a, connects_b, seal_strength);
        edge.is_sealed = false;
        edge
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

impl Default for DamageGraph {
    fn default() -> Self {
        Self::new()
    }
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
        // Bug №240: this used to preserve only `current_level` and
        // `fire_intensity`, despite the comment promising that a duplicate
        // "must not wipe live flooding/fire". Everything else in the incoming
        // node came from the ship's state with fresh defaults, so every
        // repeated call reset `is_breached`, `is_sealed`, `damage` and
        // `pump_active` to "intact / unsealed / undamaged / off" — and because
        // `node_map` still pointed at the same index, the node looked
        // preserved while its properties were gone.
        //
        // The full live state is now carried over, and the node is marked
        // damaged if it is already breached, so a rebuild cannot quietly
        // un-breach a compartment.
        if let Some(&idx) = self.node_map.get(&node.entity_id) {
            let live = self.graph[idx].clone();
            let mut merged = node;
            merged.current_level = live.current_level;
            merged.fire_intensity = live.fire_intensity;
            merged.is_breached = live.is_breached || merged.is_breached;
            merged.is_sealed = live.is_sealed;
            merged.damage = live.damage.max(merged.damage);
            merged.pump_active = live.pump_active;
            if merged.is_breached {
                // A breached compartment cannot also be a sealed, sound one.
                merged.is_sealed = false;
            }
            self.graph[idx] = merged;
            return idx;
        }
        let idx = self.graph.add_node(node.clone());
        self.node_map.insert(node.entity_id, idx);
        idx
    }

    pub fn remove_compartment(&mut self, entity_id: EntityId) -> Option<CompartmentNode> {
        if let Some(idx) = self.node_map.remove(&entity_id) {
            let removed = self.graph.remove_node(idx);
            // Bug №64: petgraph's remove_node can shift the remaining
            // NodeIndex values, leaving the maps pointing at wrong nodes.
            // Rebuild both maps from the surviving weights.
            self.rebuild_maps();
            removed
        } else {
            None
        }
    }

    pub fn add_bulkhead(&mut self, edge: BulkheadEdge) -> Option<EdgeIndex> {
        let idx_a = self.node_map.get(&edge.connects_a)?;
        let idx_b = self.node_map.get(&edge.connects_b)?;
        // Duplicate id must not orphan the old edge: remove it first.
        if let Some(old) = self.edge_map.remove(&edge.entity_id) {
            self.graph.remove_edge(old);
        }

        let edge_idx = self.graph.add_edge(*idx_a, *idx_b, edge.clone());
        self.edge_map.insert(edge.entity_id, edge_idx);
        Some(edge_idx)
    }

    pub fn remove_bulkhead(&mut self, entity_id: EntityId) -> Option<BulkheadEdge> {
        if let Some(idx) = self.edge_map.remove(&entity_id) {
            let removed = self.graph.remove_edge(idx);
            self.rebuild_maps();
            removed
        } else {
            None
        }
    }

    /// Re-derive node_map/edge_map from the surviving weights (Bug №64:
    /// petgraph indices can shift on node removal).
    fn rebuild_maps(&mut self) {
        self.node_map.clear();
        for idx in self.graph.node_indices() {
            let id = self.graph[idx].entity_id;
            self.node_map.insert(id, idx);
        }
        self.edge_map.clear();
        for edge in self.graph.edge_references() {
            let id = edge.weight().entity_id;
            self.edge_map.insert(id, edge.id());
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
            // Any connecting edge is the bulkhead between the two compartments;
            // only one is ever created per pair, so the first one is the answer.
            return self
                .graph
                .edges_connecting(*idx_a, *idx_b)
                .next()
                .map(|edge| edge.weight().entity_id);
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
            edge.health -= damage.max(0.0);
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
            edge.health = (edge.health + amount.max(0.0)).min(edge.max_health);
            if edge.health > 0.0 && edge.is_destroyed {
                // Bug №64: a repaired bulkhead used to stay OPEN forever
                // (is_destroyed cleared, but is_sealed stayed false and the
                // resistance was never restored).
                edge.is_destroyed = false;
                edge.is_sealed = true;
                edge.flow_resistance = 1.0 / edge.seal_strength.max(0.1);
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
        // Bug №64: negative amounts drained the compartment in reverse.
        if amount <= 0.0 {
            return 0.0;
        }
        if let Some(node) = self.get_compartment_mut(compartment_id) {
            let space = (node.max_capacity - node.current_level).max(0.0);
            let added = amount.min(space);
            node.current_level += added;
            added
        } else {
            0.0
        }
    }

    pub fn pump_water(&mut self, compartment_id: EntityId, amount: f32) -> f32 {
        // Bug №64: negative amounts filled the compartment in reverse.
        if amount <= 0.0 {
            return 0.0;
        }
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
        // Bug №64: negative damage healed the compartment.
        if damage <= 0.0 {
            return false;
        }
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
#[cfg(test)]
mod tests {
    use super::*;

    fn compartment(id: u64) -> CompartmentNode {
        CompartmentNode {
            entity_id: EntityId::new(id),
            name: format!("c{id}"),
            max_capacity: 100.0,
            current_level: 0.0,
            is_sealed: false,
            is_breached: false,
            pump_capacity: 10.0,
            pump_active: false,
            fire_intensity: 0.0,
            damage: 0.0,
            max_damage: 100.0,
        }
    }

    /// Bug №240: a repeated add_compartment used to keep only the water level
    /// and the fire, and reset every other live field to the incoming
    /// defaults — so a breached, damaged, pumping compartment silently became
    /// intact, sound and switched off while still being "found" in the map.
    #[test]
    fn re_adding_a_compartment_preserves_its_live_state() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));

        // Put the node into a damaged, breached, pumping, sealed, burning state.
        {
            let idx = *g.node_map.get(&EntityId::new(1)).unwrap();
            let n = &mut g.graph[idx];
            n.is_breached = true;
            n.damage = 42.0;
            n.pump_active = true;
            n.is_sealed = true;
            n.fire_intensity = 0.75;
            n.current_level = 55.0;
        }

        // Re-add it from a freshly built node (all defaults).
        g.add_compartment(compartment(1));

        let idx = *g.node_map.get(&EntityId::new(1)).unwrap();
        let n = &g.graph[idx];
        assert!((n.current_level - 55.0).abs() < 1e-5, "water level was reset");
        assert!((n.fire_intensity - 0.75).abs() < 1e-5, "fire was reset");
        assert!(n.is_breached, "a breach was silently undone");
        assert!((n.damage - 42.0).abs() < 1e-5, "damage was reset");
        assert!(n.pump_active, "the pump was switched off");
    }

    /// A breached compartment must not come back as a sealed, sound one.
    #[test]
    fn a_breached_compartment_cannot_be_resealed_by_a_rebuild() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));
        let idx = *g.node_map.get(&EntityId::new(1)).unwrap();
        g.graph[idx].is_breached = true;

        g.add_compartment(compartment(1));
        let n = &g.graph[*g.node_map.get(&EntityId::new(1)).unwrap()];
        assert!(n.is_breached);
        assert!(!n.is_sealed, "a breached compartment must not report as sealed");
    }

    /// Bug №241: a synthetic bulkhead built for the water path was sealed, and
    /// `is_passable()` is `!is_sealed || is_destroyed` — so it could never let
    /// water through, the exact opposite of what the caller's comment claimed.
    #[test]
    fn a_synthetic_bulkhead_is_passable_so_water_can_flow() {
        let open = BulkheadEdge::open(EntityId::new(1), EntityId::new(2), EntityId::new(3), 100.0);
        assert!(
            open.is_passable(),
            "a synthetic edge exists to provide a route and must start passable"
        );
        // A real, intact bulkhead is still sealed by default.
        let sealed = BulkheadEdge::new(EntityId::new(4), EntityId::new(2), EntityId::new(3), 100.0);
        assert!(!sealed.is_passable(), "an intact bulkhead stays sealed");
        // And a destroyed one opens regardless.
        let mut destroyed = sealed.clone();
        destroyed.is_destroyed = true;
        assert!(destroyed.is_passable());
    }
}