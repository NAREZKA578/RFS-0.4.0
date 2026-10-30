use petgraph::graph::{EdgeIndex, Graph, NodeIndex};
use petgraph::visit::EdgeRef;
use rfs_core::entity::EntityId;
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
        self.fill_ratio() > FLOODED_FILL_RATIO
    }

    pub fn is_critical(&self) -> bool {
        self.fill_ratio() > CRITICAL_FILL_RATIO
    }

    pub fn can_pump(&self) -> bool {
        self.pump_active && self.current_level > 0.0 && !self.is_breached
    }
}

/// Bug №727: the fill ratios that decide "flooded" and "critical".
///
/// These were two pairs of literals — `> 0.9` / `> 0.5` here and the same
/// measured against `water_level > max_water_level * x` in
/// `ship::compartment::CompartmentState` — with nothing enforcing that the two
/// readers agree. The water model and the per-ship state answer the same
/// question ("is this compartment flooded"), and if one pair edits without
/// the other, `CompartmentFlooded` events arrive at fill ratios the renderer
/// does not treat as flooded. One statement, both readers import it.
pub const FLOODED_FILL_RATIO: f32 = 0.9;
/// See [`FLOODED_FILL_RATIO`] — the "critical, flooding is winning" threshold.
pub const CRITICAL_FILL_RATIO: f32 = 0.5;

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
        self.node_map
            .get(&entity_id)
            .map(|idx| &mut self.graph[*idx])
    }

    pub fn get_bulkhead(&self, entity_id: EntityId) -> Option<&BulkheadEdge> {
        self.edge_map.get(&entity_id).map(|idx| &self.graph[*idx])
    }

    pub fn get_bulkhead_mut(&mut self, entity_id: EntityId) -> Option<&mut BulkheadEdge> {
        self.edge_map
            .get(&entity_id)
            .map(|idx| &mut self.graph[*idx])
    }

    pub fn get_connected_compartments(&self, entity_id: EntityId) -> SmallVec<[EntityId; 4]> {
        let mut result = SmallVec::new();
        if let Some(idx) = self.node_map.get(&entity_id) {
            for edge in self.graph.edges(*idx) {
                let other = if edge.source() == *idx {
                    edge.target()
                } else {
                    edge.source()
                };
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
        //
        // Bug №197: NaN passed this guard too, because every comparison
        // against NaN is false — and `f32::min` returns its *second* operand
        // when the first is NaN, so `NaN.min(space)` yielded `space` and the
        // compartment was filled to capacity in a single call. Nothing failed;
        // the ship just flooded.
        let amount = crate::finite::positive_or_zero(amount);
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
        //
        // Bug №197: as in `add_water`. `NaN.min(current_level)` returned
        // `current_level`, draining the compartment completely in one call.
        let amount = crate::finite::positive_or_zero(amount);
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
        // Bug №198: `f32::clamp` returns NaN unchanged, so a NaN intensity
        // became a NaN `fire_intensity`. Every downstream `fire_intensity <=
        // 0.0` guard is then false — "burning" — and `(target + NaN).min(1.0)`
        // yields 1.0, spreading full-intensity fire to every neighbour.
        //
        // Unusable input is refused outright rather than mapped to zero:
        // garbage must not write authoritative state, and writing zero would
        // let the same NaN *extinguish* a burning compartment. Call
        // `extinguish_fire` to clear fire deliberately.
        if !intensity.is_finite() {
            return false;
        }
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
        //
        // Bug №197: NaN slipped through and `(damage + NaN).min(max_damage)`
        // returned `max_damage`, breaching the compartment outright.
        let damage = crate::finite::positive_or_zero(damage);
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
        self.graph
            .node_indices()
            .map(|idx| (self.graph[idx].entity_id, &self.graph[idx]))
    }

    pub fn iter_edges(&self) -> impl Iterator<Item = (EntityId, &BulkheadEdge)> {
        self.graph
            .edge_references()
            .map(|edge| (edge.weight().entity_id, edge.weight()))
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
        assert!(
            (n.current_level - 55.0).abs() < 1e-5,
            "water level was reset"
        );
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
        assert!(
            !n.is_sealed,
            "a breached compartment must not report as sealed"
        );
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

    fn level(g: &DamageGraph, id: EntityId) -> f32 {
        g.get_compartment(id).unwrap().current_level
    }

    /// Bug №197: `add_water(NaN)` filled the compartment to capacity in one
    /// call, because `NaN <= 0.0` is false and `f32::min(NaN, space)` returns
    /// `space`. A NaN anywhere upstream therefore sank the ship.
    #[test]
    fn nan_water_does_not_flood_a_compartment() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));

        let added = g.add_water(EntityId::new(1), f32::NAN);

        assert_eq!(added, 0.0, "a NaN amount reported water entering");
        assert_eq!(
            level(&g, EntityId::new(1)),
            0.0,
            "a NaN amount filled the compartment"
        );
    }

    /// Bug №197, the mirror image: `pump_water(NaN)` emptied the compartment,
    /// because `f32::min(NaN, current)` returns `current`.
    #[test]
    fn nan_pump_does_not_drain_a_compartment() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));
        g.add_water(EntityId::new(1), 40.0);
        let before = level(&g, EntityId::new(1));
        assert!(before > 0.0);

        let pumped = g.pump_water(EntityId::new(1), f32::NAN);

        assert_eq!(pumped, 0.0, "a NaN amount reported water leaving");
        assert_eq!(
            level(&g, EntityId::new(1)),
            before,
            "a NaN drained the compartment"
        );
    }

    /// Both infinities must be refused too — `min` returns them as-is, which is
    /// just as unrepresentable as NaN.
    #[test]
    fn infinite_water_does_not_change_a_compartment() {
        for bad in [f32::INFINITY, f32::NEG_INFINITY] {
            let mut g = DamageGraph::new();
            g.add_compartment(compartment(1));
            assert_eq!(
                g.add_water(EntityId::new(1), bad),
                0.0,
                "{bad} flooded the compartment"
            );
            assert_eq!(level(&g, EntityId::new(1)), 0.0, "{bad} changed the level");

            g.add_water(EntityId::new(1), 10.0);
            let before = level(&g, EntityId::new(1));
            assert_eq!(
                g.pump_water(EntityId::new(1), bad),
                0.0,
                "{bad} drained the compartment"
            );
            assert_eq!(
                level(&g, EntityId::new(1)),
                before,
                "{bad} changed the level"
            );
        }
    }

    /// A refused amount must not be reported as "nothing happened" while still
    /// having moved water — the return value is what callers bill against.
    #[test]
    fn a_refused_amount_never_also_moves_water() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));
        for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY, -1.0] {
            assert_eq!(g.add_water(EntityId::new(1), bad), 0.0);
            assert_eq!(g.pump_water(EntityId::new(1), bad), 0.0);
            assert_eq!(level(&g, EntityId::new(1)), 0.0);
        }
    }

    /// Bug №198: `f32::clamp` returns NaN unchanged, so `set_fire` stored a NaN
    /// intensity that every `fire_intensity <= 0.0` guard reads as "burning".
    #[test]
    fn nan_fire_intensity_is_refused() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));

        assert!(
            !g.set_fire(EntityId::new(1), f32::NAN),
            "a NaN reported a successful ignite"
        );

        let intensity = g.get_compartment(EntityId::new(1)).unwrap().fire_intensity;
        assert!(intensity.is_finite(), "a NaN intensity was stored");
        assert_eq!(
            intensity, 0.0,
            "an unusable intensity must read as 'not burning', not as burning"
        );
    }

    /// Refusing is what makes this safe. Mapping a NaN to zero instead would let
    /// anyone who can inject a bad float put out a fire at will — on an
    /// authoritative server that is a griefing vector, not a rounding detail.
    #[test]
    fn nan_fire_does_not_extinguish_a_burning_compartment() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));
        g.set_fire(EntityId::new(1), 0.6);

        g.set_fire(EntityId::new(1), f32::NAN);

        let intensity = g.get_compartment(EntityId::new(1)).unwrap().fire_intensity;
        assert!(
            (intensity - 0.6).abs() < 1e-5,
            "an unusable intensity overwrote live fire, got {intensity}"
        );
    }

    /// Clearing fire stays possible — that is what `extinguish_fire` is for, so
    /// refusing garbage costs nothing.
    #[test]
    fn fire_can_still_be_cleared_deliberately() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));
        g.set_fire(EntityId::new(1), 0.6);

        assert!(g.extinguish_fire(EntityId::new(1)));
        assert_eq!(
            g.get_compartment(EntityId::new(1)).unwrap().fire_intensity,
            0.0
        );
    }

    #[test]
    fn infinite_fire_intensity_is_refused() {
        for bad in [f32::INFINITY, f32::NEG_INFINITY] {
            let mut g = DamageGraph::new();
            g.add_compartment(compartment(1));
            assert!(
                !g.set_fire(EntityId::new(1), bad),
                "{bad} reported a successful ignite"
            );
            assert_eq!(
                g.get_compartment(EntityId::new(1)).unwrap().fire_intensity,
                0.0
            );
        }
    }

    /// Out-of-range but finite intensities are still clamped, not refused —
    /// refusing them would change behaviour for legitimate callers.
    #[test]
    fn an_out_of_range_intensity_is_clamped_not_refused() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));

        assert!(g.set_fire(EntityId::new(1), 5.0));
        assert_eq!(
            g.get_compartment(EntityId::new(1)).unwrap().fire_intensity,
            1.0
        );

        assert!(g.set_fire(EntityId::new(1), -5.0));
        assert_eq!(
            g.get_compartment(EntityId::new(1)).unwrap().fire_intensity,
            0.0
        );
    }

    /// Bug №197 again, on the damage path: `(damage + NaN).min(max_damage)`
    /// returns `max_damage`, so a NaN blast breached the compartment outright.
    #[test]
    fn nan_damage_does_not_breach_a_compartment() {
        let mut g = DamageGraph::new();
        g.add_compartment(compartment(1));

        assert!(
            !g.apply_damage(EntityId::new(1), f32::NAN),
            "a NaN reported a hit"
        );

        let node = g.get_compartment(EntityId::new(1)).unwrap();
        assert_eq!(node.damage, 0.0, "a NaN blast dealt damage");
        assert!(!node.is_breached, "a NaN blast breached the compartment");
    }

    #[test]
    fn infinite_damage_does_not_breach_a_compartment() {
        for bad in [f32::INFINITY, f32::NEG_INFINITY] {
            let mut g = DamageGraph::new();
            g.add_compartment(compartment(1));
            assert!(
                !g.apply_damage(EntityId::new(1), bad),
                "{bad} reported a hit"
            );
            let node = g.get_compartment(EntityId::new(1)).unwrap();
            assert_eq!(node.damage, 0.0, "{bad} dealt damage");
            assert!(!node.is_breached, "{bad} breached the compartment");
        }
    }

    /// Bug №727: the thresholds are the named constants, and the pair reads
    /// the way both readers must agree on. The ordering of the constants is
    /// not asserted directly (a const-vs-const assert clippy flags as
    /// vacuous) — the fills below walk the ordering behaviourally: quarter is
    /// neither, exactly-threshold is below, 0.7 is critical-not-flooded, past
    /// 0.9 is both.
    #[test]
    fn the_thresholds_are_the_named_constants_and_the_pair_is_ordered() {
        let mut node = compartment(1);

        node.max_capacity = 100.0;
        node.current_level = 25.0;
        assert!(!node.is_critical(), "a quarter full is not critical");
        assert!(!node.is_flooded(), "a quarter full is never flooded");

        // Exactly at the threshold is below it: the predicate is strict `>`.
        node.current_level = CRITICAL_FILL_RATIO * node.max_capacity;
        assert!(!node.is_critical(), "exactly at the critical ratio is below it");
        assert!(!node.is_flooded());

        node.current_level = 70.0;
        assert!(node.is_critical(), "criticality long since passed");
        assert!(!node.is_flooded(), "seventy percent is not flooded");

        node.current_level = FLOODED_FILL_RATIO * node.max_capacity;
        assert!(!node.is_flooded(), "exactly at the flood ratio is still below it");

        node.current_level = FLOODED_FILL_RATIO * node.max_capacity + 0.01;
        assert!(node.is_flooded(), "past the named ratio it is flooded");
        assert!(node.is_critical());
    }
}
