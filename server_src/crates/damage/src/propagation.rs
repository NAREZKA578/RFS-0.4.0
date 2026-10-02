use crate::graph::{DamageGraph, CompartmentNode};
use rfs_core::entity::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet, VecDeque};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstanceType {
    Water,
    Fire,
    Smoke,
    Gas,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationConfig {
    pub water_flow_rate: f32,
    pub water_equalization_rate: f32,
    pub fire_spread_rate: f32,
    pub fire_spread_threshold: f32,
    pub smoke_spread_rate: f32,
    pub gas_spread_rate: f32,
    pub max_iterations_per_tick: usize,
}

impl Default for PropagationConfig {
    fn default() -> Self {
        Self {
            water_flow_rate: 50.0,
            water_equalization_rate: 0.1,
            fire_spread_rate: 0.2,
            fire_spread_threshold: 0.3,
            smoke_spread_rate: 10.0,
            gas_spread_rate: 5.0,
            max_iterations_per_tick: 10,
        }
    }
}

pub struct PropagationSystem {
    config: PropagationConfig,
    event_queue: VecDeque<PropagationEvent>,
    // Bug №64: Flooded/Critical were pushed every tick regardless of any
    // change. These track the last emitted state so events only fire on
    // transitions (edge-triggered).
    flooded_known: HashMap<EntityId, bool>,
    critical_known: HashMap<EntityId, bool>,
}

#[derive(Debug, Clone)]
pub struct PropagationEvent {
    pub event_type: PropagationEventType,
    pub source: EntityId,
    pub target: Option<EntityId>,
    pub amount: f32,
    pub substance: SubstanceType,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PropagationEventType {
    WaterIntake,
    WaterFlow,
    WaterPumped,
    FireIgnited,
    FireSpread,
    FireExtinguished,
    BulkheadBreached,
    BulkheadSealed,
    BulkheadDestroyed,
    CompartmentFlooded,
    CompartmentCritical,
}

impl PropagationSystem {
    pub fn new(config: PropagationConfig) -> Self {
        Self {
            config,
            event_queue: VecDeque::new(),
            flooded_known: HashMap::new(),
            critical_known: HashMap::new(),
        }
    }

    pub fn update(&mut self, graph: &mut DamageGraph, dt: f32) -> Vec<PropagationEvent> {
        let mut events = Vec::new();
        
        self.propagate_water(graph, dt, &mut events);
        self.propagate_fire(graph, dt, &mut events);
        self.process_pumps(graph, dt, &mut events);
        self.check_critical_states(graph, &mut events);

        // Bug №161: compartments that were removed or scrapped left stale
        // entries in the edge-trigger maps — a re-add then suppressed the
        // first Flooded/Critical event forever.
        self.prune_known_states(graph);
        
        events.append(&mut self.event_queue.drain(..).collect());
        events
    }

    fn prune_known_states(&mut self, graph: &DamageGraph) {
        let alive: HashSet<EntityId> = graph.iter_nodes().map(|(id, _)| id).collect();
        self.flooded_known.retain(|id, _| alive.contains(id));
        self.critical_known.retain(|id, _| alive.contains(id));
    }

    fn propagate_water(&self, graph: &mut DamageGraph, dt: f32, events: &mut Vec<PropagationEvent>) {
        // Bug №63: intake used to run inside the up-to-10 iteration loop
        // against flags captured ONCE into a stale snapshot — every breached
        // compartment flooded up to x10 per tick. It runs exactly once now.
        let breached: Vec<EntityId> = graph
            .iter_nodes()
            .filter(|(_, c)| c.is_breached && !c.is_sealed)
            .map(|(id, _)| id)
            .collect();

        for comp_id in breached {
            let intake = self.config.water_flow_rate * dt;
            let added = graph.add_water(comp_id, intake);
            if added > 0.0 {
                events.push(PropagationEvent {
                    event_type: PropagationEventType::WaterIntake,
                    source: comp_id,
                    target: None,
                    amount: added,
                    substance: SubstanceType::Water,
                });
            }
        }

        for _ in 0..self.config.max_iterations_per_tick {
            // Bug №63: one clone was reused for all iterations, so water
            // already moved this tick was "moved" again. A fresh snapshot per
            // iteration keeps the iteration count meaningful.
            let compartments: Vec<(EntityId, CompartmentNode)> = graph
                .iter_nodes()
                .map(|(id, node)| (id, node.clone()))
                .collect();

            let mut any_flow = false;

            for (comp_id, comp) in &compartments {
                if comp.current_level <= 0.0 {
                    continue;
                }

                let connected = graph.get_connected_compartments(*comp_id);

                for target_id in connected {
                    let Some(bulkhead_id) = graph.get_bulkhead_between(*comp_id, target_id) else {
                        continue;
                    };
                    let Some(bulkhead) = graph.get_bulkhead(bulkhead_id) else {
                        continue;
                    };
                    if !bulkhead.is_passable() {
                        continue;
                    }
                    let Some(source) = graph.get_compartment(*comp_id) else {
                        continue;
                    };
                    let Some(target) = graph.get_compartment(target_id) else {
                        continue;
                    };

                    let pressure_diff = source.fill_ratio() - target.fill_ratio();
                    if pressure_diff <= 0.01 {
                        continue;
                    }

                    // Bug №63: overflow past the sink's capacity used to be
                    // silently dropped — water vanished. Cap by the room.
                    let room = (target.max_capacity - target.current_level).max(0.0);
                    if room <= 0.0 {
                        continue;
                    }

                    // Bug №63: the hidden *100 was never documented and the
                    // bulkhead resistance was never consulted. Now the
                    // resistance slows the flow towards the sink.
                    let resistance = bulkhead.effective_resistance().max(0.05);
                    let flow = pressure_diff
                        * self.config.water_equalization_rate
                        * dt
                        * 100.0
                        / resistance;
                    let actual_flow = flow.min(source.current_level).min(room);

                    if actual_flow > 0.0 && actual_flow.is_finite() {
                        // Respect add_water capacity: only pump what fits,
                        // otherwise volume is destroyed on fan-in.
                        let room = graph
                            .get_compartment(target_id)
                            .map(|n| (n.max_capacity - n.current_level).max(0.0))
                            .unwrap_or(0.0);
                        let move_amt = actual_flow.min(room);
                        if move_amt > 0.0 {
                            graph.pump_water(*comp_id, move_amt);
                            graph.add_water(target_id, move_amt);

                            events.push(PropagationEvent {
                                event_type: PropagationEventType::WaterFlow,
                                source: *comp_id,
                                target: Some(target_id),
                                amount: move_amt,
                                substance: SubstanceType::Water,
                            });

                            any_flow = true;
                        }
                    }
                }
            }

            if !any_flow {
                break;
            }
        }
    }

    fn propagate_fire(&self, graph: &mut DamageGraph, dt: f32, events: &mut Vec<PropagationEvent>) {
        let compartments: Vec<(EntityId, CompartmentNode)> = graph
            .iter_nodes()
            .map(|(id, node)| (id, node.clone()))
            .collect();
        
        for (comp_id, comp) in &compartments {
            if comp.fire_intensity <= 0.0 {
                continue;
            }
            
            if comp.is_flooded() {
                let reduction = comp.fire_intensity * 0.5 * dt;
                if let Some(node) = graph.get_compartment_mut(*comp_id) {
                    node.fire_intensity = (node.fire_intensity - reduction).max(0.0);
                    if node.fire_intensity <= 0.0 {
                        events.push(PropagationEvent {
                            event_type: PropagationEventType::FireExtinguished,
                            source: *comp_id,
                            target: None,
                            amount: 0.0,
                            substance: SubstanceType::Fire,
                        });
                    }
                }
                continue;
            }
            
            let connected = graph.get_connected_compartments(*comp_id);
            
            for target_id in connected {
                if let Some(bulkhead_id) = graph.get_bulkhead_between(*comp_id, target_id) {
                    let bulkhead_passable = graph
                        .get_bulkhead(bulkhead_id)
                        .map(|b| b.is_passable())
                        .unwrap_or(false);

                    if bulkhead_passable {
                        // Bug №64: a MISSING compartment used to default to
                        // fire_intensity 1.0 and silently block the spread.
                        let Some(target) = graph.get_compartment(target_id) else {
                            continue;
                        };
                        if target.fire_intensity >= self.config.fire_spread_threshold {
                            continue;
                        }

                        // Bug №64: growth was gated by fastrand — replays and
                        // the authoritative tick became nondeterministic. A
                        // deterministic amount is added per tick instead.
                        let growth = comp.fire_intensity * self.config.fire_spread_rate * dt;
                        if growth <= 0.0 {
                            continue;
                        }
                        let new_intensity = (target.fire_intensity + growth).min(1.0);
                        if let Some(node) = graph.get_compartment_mut(target_id) {
                            node.fire_intensity = new_intensity;

                            events.push(PropagationEvent {
                                event_type: PropagationEventType::FireSpread,
                                source: *comp_id,
                                target: Some(target_id),
                                amount: new_intensity,
                                substance: SubstanceType::Fire,
                            });
                        }
                    }
                }
            }
        }
    }

    fn process_pumps(&self, graph: &mut DamageGraph, dt: f32, events: &mut Vec<PropagationEvent>) {
        let pumpers: Vec<(EntityId, bool, f32)> = graph
            .iter_nodes()
            .map(|(id, comp)| (id, comp.can_pump(), comp.pump_capacity))
            .collect();

        for (comp_id, can_pump, pump_capacity) in pumpers {
            if can_pump {
                let pump_amount = pump_capacity * dt;
                let pumped = graph.pump_water(comp_id, pump_amount);
                
                if pumped > 0.0 {
                    events.push(PropagationEvent {
                        event_type: PropagationEventType::WaterPumped,
                        source: comp_id,
                        target: None,
                        amount: pumped,
                        substance: SubstanceType::Water,
                    });
                }
            }
        }
    }

    fn check_critical_states(&mut self, graph: &DamageGraph, events: &mut Vec<PropagationEvent>) {
        // Bug №64: Flooded/Critical were emitted every tick — event flood.
        // Compare with the last reported state; only fire on transitions.
        for (comp_id, comp) in graph.iter_nodes() {
            let flooded = comp.is_flooded();
            if flooded && !self.flooded_known.get(&comp_id).copied().unwrap_or(false) {
                events.push(PropagationEvent {
                    event_type: PropagationEventType::CompartmentFlooded,
                    source: comp_id,
                    target: None,
                    amount: comp.fill_ratio(),
                    substance: SubstanceType::Water,
                });
            }
            self.flooded_known.insert(comp_id, flooded);

            // Edge-triggered Critical excluding Flooded: store (critical && !flooded)
            // so Flooded->Critical->Flooded->Critical re-fires after pump-out.
            let critical_active = comp.is_critical() && !flooded;
            if critical_active && !self.critical_known.get(&comp_id).copied().unwrap_or(false) {
                events.push(PropagationEvent {
                    event_type: PropagationEventType::CompartmentCritical,
                    source: comp_id,
                    target: None,
                    amount: comp.fill_ratio(),
                    substance: SubstanceType::Water,
                });
            }
            self.critical_known.insert(comp_id, critical_active);
        }
    }

    pub fn ignite_fire(&mut self, graph: &mut DamageGraph, compartment_id: EntityId, intensity: f32) -> bool {
        if let Some(node) = graph.get_compartment_mut(compartment_id) {
            if !node.is_flooded() {
                node.fire_intensity = intensity.clamp(0.0, 1.0);
                self.event_queue.push_back(PropagationEvent {
                    event_type: PropagationEventType::FireIgnited,
                    source: compartment_id,
                    target: None,
                    amount: intensity,
                    substance: SubstanceType::Fire,
                });
                return true;
            }
        }
        false
    }

    pub fn extinguish_fire(&mut self, graph: &mut DamageGraph, compartment_id: EntityId) -> bool {
        if graph.extinguish_fire(compartment_id) {
            self.event_queue.push_back(PropagationEvent {
                event_type: PropagationEventType::FireExtinguished,
                source: compartment_id,
                target: None,
                amount: 0.0,
                substance: SubstanceType::Fire,
            });
            true
        } else {
            false
        }
    }

    pub fn breach_compartment(&mut self, graph: &mut DamageGraph, compartment_id: EntityId) -> bool {
        if graph.breach_compartment(compartment_id) {
            // Do NOT emit CompartmentFlooded here: breach starts at water 0,
            // the edge detector below emits Flooded when fill_ratio actually
            // crosses the threshold.
            true
        } else {
            false
        }
    }

    pub fn seal_bulkhead(&mut self, graph: &mut DamageGraph, bulkhead_id: EntityId) -> bool {
        if graph.seal_bulkhead(bulkhead_id) {
            self.event_queue.push_back(PropagationEvent {
                event_type: PropagationEventType::BulkheadSealed,
                source: bulkhead_id,
                target: None,
                amount: 0.0,
                substance: SubstanceType::Water,
            });
            true
        } else {
            false
        }
    }

    pub fn unseal_bulkhead(&mut self, graph: &mut DamageGraph, bulkhead_id: EntityId) -> bool {
        if graph.unseal_bulkhead(bulkhead_id) {
            self.event_queue.push_back(PropagationEvent {
                event_type: PropagationEventType::BulkheadBreached,
                source: bulkhead_id,
                target: None,
                amount: 0.0,
                substance: SubstanceType::Water,
            });
            true
        } else {
            false
        }
    }

    pub fn damage_bulkhead(&mut self, graph: &mut DamageGraph, bulkhead_id: EntityId, damage: f32) -> bool {
        if graph.damage_bulkhead(bulkhead_id, damage) {
            if let Some(bulkhead) = graph.get_bulkhead(bulkhead_id) {
                if bulkhead.is_destroyed {
                    self.event_queue.push_back(PropagationEvent {
                        event_type: PropagationEventType::BulkheadDestroyed,
                        source: bulkhead_id,
                        target: None,
                        amount: damage,
                        substance: SubstanceType::Water,
                    });
                }
            }
            true
        } else {
            false
        }
    }

    pub fn get_pending_events(&self) -> &VecDeque<PropagationEvent> {
        &self.event_queue
    }

    pub fn clear_events(&mut self) {
        self.event_queue.clear();
    }
}

pub struct FloodingSimulator {
    graph: DamageGraph,
    propagation: PropagationSystem,
    water_sources: Vec<WaterSource>,
}

#[derive(Debug, Clone)]
pub struct WaterSource {
    pub compartment_id: EntityId,
    pub flow_rate: f32,
    pub is_active: bool,
}

impl FloodingSimulator {
    pub fn new(graph: DamageGraph, config: PropagationConfig) -> Self {
        Self {
            graph,
            propagation: PropagationSystem::new(config),
            water_sources: Vec::new(),
        }
    }

    pub fn add_water_source(&mut self, compartment_id: EntityId, flow_rate: f32) {
        self.water_sources.push(WaterSource {
            compartment_id,
            flow_rate,
            is_active: true,
        });
    }

    pub fn remove_water_source(&mut self, compartment_id: EntityId) {
        self.water_sources.retain(|s| s.compartment_id != compartment_id);
    }

    pub fn step(&mut self, dt: f32) -> Vec<PropagationEvent> {
        for source in &self.water_sources {
            if source.is_active {
                self.graph.add_water(source.compartment_id, source.flow_rate * dt);
            }
        }
        
        self.propagation.update(&mut self.graph, dt)
    }

    pub fn graph(&self) -> &DamageGraph {
        &self.graph
    }

    pub fn graph_mut(&mut self) -> &mut DamageGraph {
        &mut self.graph
    }

    pub fn get_compartment_state(&self, compartment_id: EntityId) -> Option<CompartmentState> {
        self.graph.get_compartment(compartment_id).map(|c| CompartmentState {
            entity_id: c.entity_id,
            name: c.name.clone(),
            water_level: c.current_level,
            max_water_level: c.max_capacity,
            fill_ratio: c.fill_ratio(),
            is_sealed: c.is_sealed,
            is_breached: c.is_breached,
            fire_intensity: c.fire_intensity,
            pump_active: c.pump_active,
            pump_capacity: c.pump_capacity,
        })
    }

    pub fn total_water_volume(&self) -> f32 {
        self.graph.all_compartments().iter().map(|c| c.current_level).sum()
    }

    pub fn flooded_compartment_count(&self) -> usize {
        self.graph.all_compartments().iter().filter(|c| c.is_flooded()).count()
    }

    pub fn critical_compartment_count(&self) -> usize {
        self.graph.all_compartments().iter().filter(|c| c.is_critical()).count()
    }

    pub fn is_ship_lost(&self) -> bool {
        let total_capacity: f32 = self.graph.all_compartments().iter().map(|c| c.max_capacity).sum();
        let total_water: f32 = self.graph.all_compartments().iter().map(|c| c.current_level).sum();

        // Bug №83: 0/0 = NaN never compares > 0.7 — an empty graph meant the
        // ship could never be counted as lost.
        if total_capacity <= 0.0 {
            return false;
        }
        total_water / total_capacity > 0.7
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompartmentState {
    pub entity_id: EntityId,
    pub name: String,
    pub water_level: f32,
    pub max_water_level: f32,
    pub fill_ratio: f32,
    pub is_sealed: bool,
    pub is_breached: bool,
    pub fire_intensity: f32,
    pub pump_active: bool,
    pub pump_capacity: f32,
}