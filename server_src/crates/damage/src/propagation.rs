use crate::graph::{DamageGraph, CompartmentNode};
use rfs_core::entity::EntityId;
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

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
        }
    }

    pub fn update(&mut self, graph: &mut DamageGraph, dt: f32) -> Vec<PropagationEvent> {
        let mut events = Vec::new();
        
        self.propagate_water(graph, dt, &mut events);
        self.propagate_fire(graph, dt, &mut events);
        self.process_pumps(graph, dt, &mut events);
        self.check_critical_states(graph, &mut events);
        
        events.append(&mut self.event_queue.drain(..).collect());
        events
    }

    fn propagate_water(&self, graph: &mut DamageGraph, dt: f32, events: &mut Vec<PropagationEvent>) {
        let compartments: Vec<(EntityId, CompartmentNode)> = graph
            .iter_nodes()
            .map(|(id, node)| (id, node.clone()))
            .collect();
        
        for _ in 0..self.config.max_iterations_per_tick {
            let mut any_flow = false;
            
            for (comp_id, comp) in &compartments {
                if comp.is_breached && !comp.is_sealed {
                    let intake = self.config.water_flow_rate * dt;
                    let added = graph.add_water(*comp_id, intake);
                    if added > 0.0 {
                        events.push(PropagationEvent {
                            event_type: PropagationEventType::WaterIntake,
                            source: *comp_id,
                            target: None,
                            amount: added,
                            substance: SubstanceType::Water,
                        });
                    }
                }
            }
            
            for (comp_id, comp) in &compartments {
                if comp.current_level <= 0.0 {
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
                            let target_fill = graph
                                .get_compartment(target_id)
                                .map(|target| target.fill_ratio())
                                .unwrap_or(0.0);

                            let pressure_diff = comp.fill_ratio() - target_fill;

                            if pressure_diff > 0.01 {
                                let flow = pressure_diff * self.config.water_equalization_rate * dt * 100.0;
                                let actual_flow = flow.min(comp.current_level);

                                if actual_flow > 0.0 {
                                    graph.pump_water(*comp_id, actual_flow);
                                    graph.add_water(target_id, actual_flow);

                                    events.push(PropagationEvent {
                                        event_type: PropagationEventType::WaterFlow,
                                        source: *comp_id,
                                        target: Some(target_id),
                                        amount: actual_flow,
                                        substance: SubstanceType::Water,
                                    });

                                    any_flow = true;
                                }
                            }
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
                        let target_intensity = graph
                            .get_compartment(target_id)
                            .map(|target| target.fire_intensity)
                            .unwrap_or(1.0);

                        if target_intensity < self.config.fire_spread_threshold {
                            let spread_chance = comp.fire_intensity * self.config.fire_spread_rate * dt;

                            if fastrand::f32() < spread_chance {
                                let new_intensity = (target_intensity + 0.1).min(1.0);
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

    fn check_critical_states(&self, graph: &DamageGraph, events: &mut Vec<PropagationEvent>) {
        for (comp_id, comp) in graph.iter_nodes() {
            if comp.is_flooded() {
                events.push(PropagationEvent {
                    event_type: PropagationEventType::CompartmentFlooded,
                    source: comp_id,
                    target: None,
                    amount: comp.fill_ratio(),
                    substance: SubstanceType::Water,
                });
            } else if comp.is_critical() {
                events.push(PropagationEvent {
                    event_type: PropagationEventType::CompartmentCritical,
                    source: comp_id,
                    target: None,
                    amount: comp.fill_ratio(),
                    substance: SubstanceType::Water,
                });
            }
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
            self.event_queue.push_back(PropagationEvent {
                event_type: PropagationEventType::CompartmentFlooded,
                source: compartment_id,
                target: None,
                amount: 1.0,
                substance: SubstanceType::Water,
            });
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