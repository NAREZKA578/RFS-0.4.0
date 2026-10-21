//! The command plane: a client command packet becomes honest mutations of the
//! simulation, with a refusal reason for everything that did not happen.
//!
//! Split out of the binary (plan H5). The command path (`translate_command`)
//! and the continuous input path (`handle_station_interaction`) share the same
//! ceilings and the same refusal wording, so they live beside each other.

use rfs_core::entity::{EntityId, StationType};
use rfs_core::packet::{
    ChatChannel, CommandPacket, GameEvent as CoreGameEvent, ProjectileType, ServerCommand,
    StationAction, StationInteraction,
};
use rfs_net::*;
use rfs_ship::ship::Ship;
use rfs_ship::station::{DefaultBehavior, Station};
use rfs_sim::tick::TickSystem;

use crate::match_::current_helm_occupant;
use crate::tick::{clamp_axis, defer_shot, spawn_projectile, DeferredShot};

/// Bug №604 (analysis 3.1.3): the repair amount arrives as an `f32` from the
/// trusted-ish client. `f32::INFINITY` used to sail through `.max(0.0)` and
/// `.min(max_health)`, turning one packet into a full repair. One answer for
/// both paths: the command path and the input path clamp to the same ceiling.
pub const MAX_REPAIR_PER_ACTION: f32 = 25.0;

/// Bug №634: chat text is client-supplied and is re-broadcast to other
/// players as-is. Without a ceiling one packet costs every recipient's
/// outbox the whole string — and `broadcast_event` has no per-connection
/// queue bound (Bug №217 territory, still open). 200 chars is a sentence,
/// well under the round-trip budget of anything the ack waits for.
pub const MAX_CHAT_MESSAGE_LEN: usize = 200;

/// Applies a station action, returning a reason if the shot was refused.
pub fn handle_station_interaction(
    sim: &TickSystem,
    ship: &Ship,
    player_id: EntityId,
    interaction: &StationInteraction,
) -> Option<String> {
    // Bug №59: only the occupant may operate a station — the old code let
    // any crew member yaw/fire/reload any gun of their ship without ever
    // entering the station.
    let occupied_by = ship
        .get_state()
        .station_states
        .get(&interaction.station_id)
        .and_then(|s| s.occupant);
    if occupied_by != Some(player_id) {
        return None;
    }

    let station_compartment = || {
        ship.get_station(interaction.station_id)
            .map(|s| s.compartment_id())
            .unwrap_or(EntityId::nil())
    };

    match interaction.action {
        StationAction::SetYaw => {
            if let Some((_, pitch)) = ship.station_angles(interaction.station_id) {
                ship.set_station_target_angles(interaction.station_id, interaction.value, pitch);
            }
        }
        StationAction::SetPitch => {
            if let Some((yaw, _)) = ship.station_angles(interaction.station_id) {
                ship.set_station_target_angles(interaction.station_id, yaw, interaction.value);
            }
        }
        StationAction::SetThrottle => {
            ship.set_throttle(clamp_axis(interaction.value));
        }
        StationAction::SetRudder => {
            ship.set_rudder(clamp_axis(interaction.value));
        }
        StationAction::Fire => {
            if let Some(result) = ship.try_fire_station(interaction.station_id, None) {
                // Bug №196: a refused spawn used to be dropped silently, so the
                // shooter saw a normal frame and no shell.
                if let Err(rejected) = spawn_projectile(sim, ship, interaction.station_id, result) {
                    return Some(format!("shot refused: {}", rejected.as_str()));
                }
            }
        }
        StationAction::Reload => {
            if let Some(station) = ship.get_station(interaction.station_id) {
                ship.reload_station(interaction.station_id, station.config().default_ammo_type);
            }
        }
        StationAction::SetAmmoType => {
            // Bug №192: the value arrived as an `f32` and went through a bare
            // `as u8`. That cast saturates rather than rejecting, so `-3.0`
            // and `NaN` both became ammo type 0 — a *valid* cannonball — and
            // an out-of-range byte became 255, which the old `_ => Cannonball`
            // catch-all silently fired anyway. So the range is checked on the
            // float, before the cast, and it has to be a whole number: `5.5`
            // is not a shell.
            if !interaction.value.is_finite() || interaction.value.fract() != 0.0 {
                return Some(format!(
                    "ammo type {:?} is not a whole number",
                    interaction.value
                ));
            }
            let requested = interaction.value as i64;
            if !(0..=255).contains(&requested) || !Station::is_valid_ammo_type(requested as u8) {
                return Some(format!("unknown ammo type {requested}"));
            }
            ship.reload_station(interaction.station_id, requested as u8);
        }
        // Bug №157: engineering controls were a dead `_ => {}` — a pump
        // station could never pump, damage control never repaired nor
        // extinguished, bulkheads could not be opened or sealed.
        StationAction::PumpWater => {
            ship.toggle_station_pump(interaction.station_id);
        }
        StationAction::SealBulkhead => {
            let comp_id = station_compartment();
            if !comp_id.is_nil() {
                for b in ship
                    .get_state()
                    .compartment_states
                    .get(&comp_id)
                    .map(|c| c.bulkhead_states.clone())
                    .unwrap_or_default()
                {
                    ship.set_bulkhead_seal(b.entity_id, true);
                }
            }
        }
        StationAction::OpenBulkhead => {
            let comp_id = station_compartment();
            if !comp_id.is_nil() {
                for b in ship
                    .get_state()
                    .compartment_states
                    .get(&comp_id)
                    .map(|c| c.bulkhead_states.clone())
                    .unwrap_or_default()
                {
                    ship.set_bulkhead_seal(b.entity_id, false);
                }
            }
        }
        StationAction::Repair => {
            // Bug №604 (analysis 3.1.3): `interaction.value.max(0.0)` let
            // `f32::INFINITY` reach `.min(max_health)`, and one packet produced
            // a full repair. Non-finite is refused (and reported, №196 style);
            // finite values are clamped to the same per-action ceiling the
            // command path uses.
            if !interaction.value.is_finite() {
                return Some(format!(
                    "repair amount {:?} is not finite",
                    interaction.value
                ));
            }
            // Bug №633: the per-post cooldown lives in `Station::repair`; here
            // it only gets a reported reason instead of a silent no-op.
            if !ship.repair_station(
                interaction.station_id,
                interaction.value.clamp(0.0, MAX_REPAIR_PER_ACTION),
            ) && ship
                .station_repair_cooldown(interaction.station_id)
                .is_some_and(|left| left > 0.0)
            {
                return Some("repair is still on cooldown".into());
            }
        }
        StationAction::ExtinguishFire => {
            // Bug №607: `extinguish_compartment` used to return `true` for any
            // existing compartment, so this path acked nothing. It now reports
            // the cold case, and this names it instead of swallowing it.
            if !ship.extinguish_compartment(station_compartment()) {
                return Some("nothing is burning there".into());
            }
        }
    }
    None
}

pub fn handle_command(
    net: &NetServer,
    sim: &TickSystem,
    ship: &Ship,
    connection_id: u32,
    player_id: EntityId,
    pending: &mut Vec<DeferredShot>,
    command: &CommandPacket,
) {
    let (success, error) = translate_command(sim, ship, player_id, pending, &command.command);

    // Bug №153: StationOccupied/StationVacated were never pushed — broadcast
    // the real, command-driven transition on success.
    if success {
        match &command.command {
            ServerCommand::EnterStation { station, .. } => {
                net.broadcast_event(CoreGameEvent::StationOccupied {
                    station: *station,
                    player: player_id,
                });
            }
            ServerCommand::ExitStation { .. } => {
                let occupied = ship
                    .get_state()
                    .station_states
                    .iter()
                    .find(|(_, s)| s.occupant == Some(player_id))
                    .map(|(id, _)| *id);
                if let Some(sid) = occupied {
                    net.broadcast_event(CoreGameEvent::StationVacated {
                        station: sid,
                        player: player_id,
                    });
                }
            }
            // Bug №607: broadcast the extinguish moment, matching how the
            // occupancy commands above announce theirs. `FireStarted` is pushed
            // from the tick (it is a collision outcome); the extinguish is a
            // command, so it is announced here where the command succeeded.
            ServerCommand::ExtinguishFire { compartment } => {
                net.broadcast_event(CoreGameEvent::FireExtinguished {
                    compartment: *compartment,
                });
            }
            // Bug №634: the accepted message goes out to the audience the
            // interest manager picks. The author is the authenticated player
            // of this connection, not the client-supplied field.
            ServerCommand::ChatMessage {
                // Spoofable: the event's author is the authenticated player.
                player: _,
                message,
                channel,
            } => {
                net.broadcast_event(CoreGameEvent::ChatReceived {
                    player: player_id,
                    message: message.clone(),
                    channel: *channel,
                });
            }
            _ => {}
        }
    }

    net.send_command_ack(connection_id, command.command_id, success, error);
}

pub fn translate_command(
    sim: &TickSystem,
    ship: &Ship,
    player_id: EntityId,
    pending: &mut Vec<DeferredShot>,
    command: &ServerCommand,
) -> (bool, Option<String>) {
    // Bug №59: a command on a station is only valid for its occupant.
    let commands_station = |station_id: &EntityId| {
        ship.get_state()
            .station_states
            .get(station_id)
            .and_then(|s| s.occupant)
            == Some(player_id)
    };
    // Bug №59: steering follows the same helm arbitration as inputs.
    let stance = || {
        let helm = current_helm_occupant(ship);
        helm.is_none() || helm == Some(player_id)
    };
    // Bug №604 (analysis 3.1.2 + 3.1.4): the six engineering commands checked
    // nothing — not occupancy, not the type of the station the player sat at,
    // so the helmsman could seal bulkheads, run pumps and repair the ship. Each
    // action now demands the authenticated player be the occupant of a station
    // whose type can actually perform it.
    let commands_engineering = |allowed: &[StationType]| {
        ship.get_state()
            .station_states
            .values()
            .any(|s| s.occupant == Some(player_id) && allowed.contains(&s.station_type))
    };

    match command {
        ServerCommand::SetShipThrottle {
            ship: target,
            throttle,
        } => {
            if *target == ship.entity_id() && stance() {
                ship.set_throttle(*throttle);
                (true, None)
            } else {
                (
                    false,
                    Some(if *target == ship.entity_id() {
                        "not at the helm".into()
                    } else {
                        "unknown ship".into()
                    }),
                )
            }
        }
        ServerCommand::SetShipRudder {
            ship: target,
            rudder,
        } => {
            if *target == ship.entity_id() && stance() {
                ship.set_rudder(*rudder);
                (true, None)
            } else {
                (
                    false,
                    Some(if *target == ship.entity_id() {
                        "not at the helm".into()
                    } else {
                        "unknown ship".into()
                    }),
                )
            }
        }
        ServerCommand::FireWeapon {
            station,
            target_pos,
        } => {
            if !commands_station(station) {
                (false, Some("station not occupied by you".into()))
            } else {
                // Bug №154: target_pos used to be ignored entirely — the
                // client could aim anywhere and the gun fired wherever it
                // happened to point. The aim is installed first; the shot
                // follows only once the turret is actually there.
                //
                // Bug №630: aiming and firing in the same tick sent the shell
                // along the orientation the turret had BEFORE slewing, so the
                // first shot of every engagement went where the gun was
                // standing, not where the gunner asked. A shot whose turret is
                // not yet on target is deferred (resolved per tick in
                // `resolve_deferred_shots`): it fires the tick the aim
                // completes, and it is refused at the deadline — a turret that
                // cannot get there must not keep the request forever. A shot
                // whose turret already points at the target fires immediately,
                // exactly as before.
                let mut aimed = false;
                if let (Some(target), Some(st_meta)) = (target_pos, ship.get_station(*station)) {
                    if target.is_finite() {
                        // Bug №664 (remainder): the loaded round names the
                        // ballistic family the solution is priced for; the
                        // family decides whether the aim is a straight line
                        // (close targets, underwater ammunition) or an arc
                        // solved with the very integrator the shell will fly.
                        let projectile_type = ship
                            .get_state()
                            .station_states
                            .get(station)
                            .and_then(|s| Station::projectile_type_for(s.ammo_type))
                            .unwrap_or(ProjectileType::Cannonball);
                        let (yaw, pitch) = crate::tick::ballistic_aim(
                            ship,
                            st_meta.local_transform().position,
                            *target,
                            projectile_type,
                            sim.ballistics(),
                        );
                        ship.set_station_target_angles(*station, yaw, pitch);
                        aimed = true;
                    }
                }
                if aimed && !ship.station_aim_reached(*station) {
                    defer_shot(pending, ship.entity_id(), *station);
                    (true, None)
                } else if let Some(result) = ship.try_fire_station(*station, None) {
                    // Bug №196: this is the command path, so the ack can carry
                    // the reason. The spawn used to be dropped silently, so a
                    // shot refused by the projectile cap was acked as success
                    // and the player saw a normal frame with no shell.
                    match spawn_projectile(sim, ship, *station, result) {
                        Ok(()) => (true, None),
                        Err(rejected) => {
                            (false, Some(format!("shot refused: {}", rejected.as_str())))
                        }
                    }
                } else {
                    (false, Some("weapon not ready".into()))
                }
            }
        }
        ServerCommand::ReloadWeapon { station, ammo_type } => {
            if !commands_station(station) {
                (false, Some("station not occupied by you".into()))
            } else if !Station::is_valid_ammo_type(*ammo_type) {
                // Bug №192: an unknown ammo type used to be accepted and then
                // fired as a cannonball by the mapping's catch-all. Reporting it
                // here is the point: an ack that says "ok" for a request that
                // loaded nothing is how this stayed invisible.
                (false, Some(format!("unknown ammo type {ammo_type}")))
            } else {
                ship.reload_station(*station, *ammo_type);
                (true, None)
            }
        }
        // Bug №157: the Ship engineering commands were a dead
        // `_ => "command not implemented"` — flooding could not be repaired,
        // fires not extinguished, bulkheads not sealed/open, pumps not run.
        ServerCommand::SealBulkhead { bulkhead } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.set_bulkhead_seal(*bulkhead, true) {
                (true, None)
            } else {
                (false, Some("bulkhead not found".into()))
            }
        }
        ServerCommand::OpenBulkhead { bulkhead } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.set_bulkhead_seal(*bulkhead, false) {
                (true, None)
            } else {
                (false, Some("bulkhead not found".into()))
            }
        }
        ServerCommand::SetCompartmentSeal {
            compartment,
            sealed,
        } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.set_compartment_seal(*compartment, *sealed) {
                (true, None)
            } else {
                (false, Some("compartment not found".into()))
            }
        }
        ServerCommand::ActivatePump { pump } => {
            if !commands_engineering(&[StationType::Pump]) {
                (false, Some("not at a pump station".into()))
            } else if ship.set_station_pump(*pump, true) {
                (true, None)
            } else {
                (false, Some("pump not found".into()))
            }
        }
        ServerCommand::DeactivatePump { pump } => {
            if !commands_engineering(&[StationType::Pump]) {
                (false, Some("not at a pump station".into()))
            } else if ship.set_station_pump(*pump, false) {
                (true, None)
            } else {
                (false, Some("pump not found".into()))
            }
        }
        ServerCommand::RepairStation { station } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.repair_station(*station, MAX_REPAIR_PER_ACTION) {
                (true, None)
            } else if ship
                .station_repair_cooldown(*station)
                .is_some_and(|left| left > 0.0)
            {
                // Bug №633: an exhausted window is not the same refusal as a
                // missing station — the client must see which one it is.
                (false, Some("repair is still on cooldown".into()))
            } else {
                (false, Some("station not found".into()))
            }
        }
        ServerCommand::ExtinguishFire { compartment } => {
            if !commands_engineering(&[StationType::DamageControl]) {
                (false, Some("not at a damage-control station".into()))
            } else if ship.extinguish_compartment(*compartment) {
                (true, None)
            } else if ship
                .get_state()
                .compartment_states
                .contains_key(compartment)
            {
                // Bug №607: the compartment exists but was not on fire, so the
                // old "compartment not found" reason was both wrong and hid the
                // fact that nothing happened. `extinguish_compartment` now
                // reports the cold case as a failure; name it.
                (false, Some("nothing is burning there".into()))
            } else {
                (false, Some("compartment not found".into()))
            }
        }
        ServerCommand::SetStationBehavior { station, behavior } => {
            if !commands_station(station) {
                (false, Some("station not occupied by you".into()))
            } else {
                // The wire code ordering lives with the ship crate; an unknown
                // code is refused here rather than defaulted, so a typo on
                // either side is visible in the ack instead of flipping a gun
                // off or on unnoticed.
                match DefaultBehavior::from_code(*behavior) {
                    Some(behavior) => {
                        if ship.set_station_behavior(*station, behavior) {
                            (true, None)
                        } else {
                            (false, Some("station not found".into()))
                        }
                    }
                    None => (false, Some(format!("unknown behavior code {behavior}"))),
                }
            }
        }
        ServerCommand::EnterStation { station, .. } => {
            // Auth: station is occupied by the authenticated connection player,
            // never by the client-supplied `player` field (spoofable).
            if ship.occupy_station(*station, player_id) {
                (true, None)
            } else {
                (false, Some("cannot occupy station".into()))
            }
        }
        ServerCommand::ExitStation { .. } => {
            // Auth: only the authenticated player can vacate their own station.
            let occupied = ship
                .get_state()
                .station_states
                .iter()
                .find(|(_, s)| s.occupant == Some(player_id))
                .map(|(id, _)| *id);
            if let Some(sid) = occupied {
                ship.vacate_station(sid);
                (true, None)
            } else {
                (false, Some("player is not in a station".into()))
            }
        }
        ServerCommand::ChatMessage {
            // Spoofable: the authenticated player replaces whatever the
            // client claimed to be.
            player: _,
            message,
            channel,
        } => {
            // Auth: the sending connection's authenticated player authors the
            // message; the client-supplied `player` field is spoofable, same
            // rule as `EnterStation`. The narrower channels are not refused
            // out of grumpiness: `Team` needs a team concept the interest
            // manager does not have, and `Whisper` has no target field in the
            // wire command to begin with — accepting either would broadcast
            // to the wrong audience while acking success.
            if !matches!(channel, ChatChannel::All | ChatChannel::Ship) {
                (false, Some("chat channel not supported".into()))
            } else if message.chars().count() > MAX_CHAT_MESSAGE_LEN {
                (
                    false,
                    Some(format!("message exceeds {MAX_CHAT_MESSAGE_LEN} characters")),
                )
            } else {
                (true, None)
            }
        }
        ServerCommand::Respawn { .. } => {
            // Bug №634: this command is declared in the wire protocol and
            // nothing implements it. The catch-all used to answer it, which
            // made a declared-but-absent feature look like a forgotten match
            // arm rather than a decision.
            //
            // The decision here is that respawn CANNOT be implemented yet:
            // nothing in the simulation marks a player dead (the `PlayerDied`
            // event is declared in the protocol and sent by nobody), so the
            // server cannot tell a dead player asking to come back from a live
            // one, and "respawn" would mean recreating a player who is
            // playing. The honest answer is a refusal that says so; the
            // question of what respawn means — where the player returns, after
            // how long, what it costs — is in the bug record for the owner.
            (false, Some("respawn is not supported yet".into()))
        }
        _ => (false, Some("command not implemented".into())),
    }
}

#[cfg(test)]
mod respawn_command_tests {
    //! Bug №634: `Respawn` is declared in the wire protocol and implemented
    //! nowhere. The catch-all answered it with "command not implemented",
    //! which reads as a forgotten match arm rather than a decision. It now has
    //! its own arm with an honest reason.

    use super::*;
    use crate::match_::SHIP_ID;
    use rfs_damage::damage::DamageSystem;
    use rfs_ship::loader::create_default_frigate;
    use rfs_ship::ship::ShipConfig;
    use std::sync::Arc;

    fn ship() -> Ship {
        let ship_config: ShipConfig = create_default_frigate().into();
        Ship::new(
            ship_config,
            EntityId::new(SHIP_ID),
            Arc::new(DamageSystem::new()),
        )
    }

    #[test]
    fn respawn_is_refused_with_a_reason_of_its_own() {
        let sim = TickSystem::new();
        let ship = ship();
        let mut pending = Vec::new();
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            EntityId::new(1),
            &mut pending,
            &ServerCommand::Respawn {
                player: EntityId::new(1),
            },
        );
        assert!(!ok, "respawn must not be reported as done");
        let reason = reason.expect("a refusal must name its reason");
        assert!(
            reason.contains("respawn"),
            "the refusal must name the command it refuses, got: {reason}"
        );
        assert_ne!(
            reason, "command not implemented",
            "the catch-all answer is exactly what this command must not give"
        );
    }
}

#[cfg(test)]
mod engineering_command_tests {
    use super::*;
    use crate::match_::SHIP_ID;
    use rfs_core::math::Vec3f;
    use rfs_damage::damage::DamageSystem;
    use rfs_ship::loader::create_default_frigate;
    use rfs_ship::ship::ShipConfig;
    use std::sync::Arc;

    fn ship() -> Ship {
        let ship_config: ShipConfig = create_default_frigate().into();
        Ship::new(
            ship_config,
            EntityId::new(SHIP_ID),
            Arc::new(DamageSystem::new()),
        )
    }

    fn station_of(ship: &Ship, kind: StationType) -> EntityId {
        ship.get_state()
            .station_states
            .iter()
            .find(|(_, s)| s.station_type == kind)
            .map(|(id, _)| *id)
            .expect("the default frigate has one of each station type")
    }

    fn some_bulkhead(ship: &Ship) -> EntityId {
        ship.get_state()
            .compartment_states
            .values()
            .flat_map(|c| c.bulkhead_states.iter())
            .map(|b| b.entity_id)
            .next()
            .expect("the default frigate has bulkheads")
    }

    /// Sits the fixture player at a seat of `kind`, then runs the command
    /// exactly the way `handle_command` does. The player is vacated first
    /// because the ship keeps the one-player-one-station invariant.
    fn seated_command(
        ship: &Ship,
        kind: StationType,
        command: &ServerCommand,
    ) -> (bool, Option<String>) {
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        for (id, s) in ship.get_state().station_states {
            if s.occupant == Some(player) {
                ship.vacate_station(id);
            }
        }
        let station = station_of(ship, kind);
        assert!(
            ship.occupy_station(station, player),
            "fixture could not sit at {kind:?}"
        );
        translate_command(&sim, ship, player, &mut Vec::new(), command)
    }

    /// Bug №604 (3.1.2): the six engineering commands checked nothing, so a
    /// player seated at no station at all could seal bulkheads.
    #[test]
    fn a_player_at_no_station_cannot_seal_a_bulkhead() {
        let ship = ship();
        let sim = TickSystem::new();
        let bulkhead = some_bulkhead(&ship);
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            EntityId::new(42),
            &mut Vec::new(),
            &ServerCommand::SealBulkhead { bulkhead },
        );
        assert!(!ok, "an unseated player sealed a bulkhead");
        assert_eq!(reason.as_deref(), Some("not at a damage-control station"));
    }

    /// Bug №604 (3.1.4): occupancy alone was never the point — the type of the
    /// seat has to matter, or the helmsman runs the whole damage-control
    /// routine.
    #[test]
    fn the_helmsman_cannot_seal_a_bulkhead() {
        let ship = ship();
        let bulkhead = some_bulkhead(&ship);
        let (ok, reason) = seated_command(
            &ship,
            StationType::Helm,
            &ServerCommand::SealBulkhead { bulkhead },
        );
        assert!(!ok, "the helmsman sealed a bulkhead");
        assert_eq!(reason.as_deref(), Some("not at a damage-control station"));
    }

    /// The fix must not cost the feature: the seat built for the job still
    /// works on every one of its commands.
    #[test]
    fn damage_control_still_seals_repairs_and_extinguishes() {
        let ship = ship();
        let bulkhead = some_bulkhead(&ship);
        let gun = station_of(&ship, StationType::Gun);
        let compartment = *ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .expect("the default frigate has compartments");

        // Bug №607: extinguishing only succeeds if something was burning, so
        // the fixture lights the compartment first. This case used to pass on a
        // cold compartment — that was the defect, not the feature.
        assert!(
            ship.ignite_compartment(compartment, 0.5),
            "the fixture could not light the compartment"
        );

        let cases = [
            ("seal", ServerCommand::SealBulkhead { bulkhead }),
            ("open", ServerCommand::OpenBulkhead { bulkhead }),
            ("repair", ServerCommand::RepairStation { station: gun }),
            ("extinguish", ServerCommand::ExtinguishFire { compartment }),
        ];
        for (label, command) in cases {
            let (ok, reason) = seated_command(&ship, StationType::DamageControl, &command);
            assert!(ok, "damage control lost {label}: {reason:?}");
        }
    }

    /// Bug №633 (command path): a second RepairStation inside the cooldown
    /// window must be refused with the honest reason. The station exists —
    /// the window does not — so "station not found" would send the client
    /// off to look for a problem that is not there.
    #[test]
    fn a_second_repair_command_within_the_window_is_refused_honestly() {
        let ship = ship();
        let gun = station_of(&ship, StationType::Gun);
        // Wound the post so a refusal has measurable consequences.
        let mut state = ship.get_state();
        state
            .station_states
            .get_mut(&gun)
            .expect("seat exists")
            .health = 100.0;
        ship.apply_state(state);

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::RepairStation { station: gun },
        );
        assert!(ok, "the first repair must go through: {reason:?}");

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::RepairStation { station: gun },
        );
        assert!(!ok, "the immediate second repair went through");
        assert_eq!(
            reason.as_deref(),
            Some("repair is still on cooldown"),
            "the refusal must name the cooldown, not a missing station"
        );
        assert_eq!(
            ship.get_state().station_states[&gun].health,
            100.0 + MAX_REPAIR_PER_ACTION,
            "the refused repair must not heal"
        );
    }

    /// Bug №634: chat used to fall into `_ => "command not implemented"`.
    /// Acceptance is honest in both directions: an in-range All/Ship message
    /// passes, an over-long one is refused with the named limit, and the
    /// channels the server cannot route are refused instead of broadcast to
    /// the wrong audience while acking success.
    #[test]
    fn a_chat_message_passes_and_is_refused_honestly() {
        let ship = ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);

        let (ok, reason) = translate_command(
            &sim,
            &ship,
            player,
            &mut Vec::new(),
            &ServerCommand::ChatMessage {
                player,
                message: "all is well".into(),
                channel: ChatChannel::All,
            },
        );
        assert!(ok, "a normal all-chat message: {reason:?}");

        // One over the ceiling must fail, and the refusal must name the
        // number — that is the only way the client can trim without guessing.
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            player,
            &mut Vec::new(),
            &ServerCommand::ChatMessage {
                player,
                message: "x".repeat(MAX_CHAT_MESSAGE_LEN + 1),
                channel: ChatChannel::All,
            },
        );
        assert!(!ok, "an oversized message went through");
        assert_eq!(
            reason.as_deref(),
            Some("message exceeds 200 characters"),
            "the limit must be named in the refusal"
        );

        // Exactly at the ceiling stays legal, on the narrow channel too.
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            player,
            &mut Vec::new(),
            &ServerCommand::ChatMessage {
                player,
                message: "x".repeat(MAX_CHAT_MESSAGE_LEN),
                channel: ChatChannel::Ship,
            },
        );
        assert!(ok, "a full-length ship message: {reason:?}");

        for channel in [ChatChannel::Team, ChatChannel::Whisper] {
            let (ok, reason) = translate_command(
                &sim,
                &ship,
                player,
                &mut Vec::new(),
                &ServerCommand::ChatMessage {
                    player,
                    message: "hi".into(),
                    channel,
                },
            );
            assert!(!ok, "channel {channel:?} was accepted");
            assert_eq!(
                reason.as_deref(),
                Some("chat channel not supported"),
                "channel {channel:?} must name the refusal"
            );
        }
    }

    /// A target the turret has not reached defers the shot instead of firing
    /// it along the orientation the turret happens to be holding. The ack is
    /// honest about the outcome: accepted (the request was valid), not "shot
    /// made" — the shell only exists once the turret arrives, which the
    /// deferral resolution owns.
    #[test]
    fn a_fire_command_at_an_unreached_target_is_deferred() {
        let ship = ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        let gun = station_of(&ship, StationType::Gun);
        assert!(ship.occupy_station(gun, player));
        let ammo_before = ship.get_state().station_states[&gun].ammo_count;

        let muzzle_local = ship
            .get_station(gun)
            .expect("the tier has a gun station")
            .local_transform()
            .position;
        // Through local_to_world so the answer is exact whatever the ship's
        // world transform: world_to_local(local_to_world(L)) == L.
        let aim_local =
            muzzle_local + Vec3f::new(100.0 * 2.0_f32.sin(), 0.0, 100.0 * 2.0_f32.cos());
        let target = ship.local_to_world(aim_local);

        let mut pending = Vec::new();
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            player,
            &mut pending,
            &ServerCommand::FireWeapon {
                station: gun,
                target_pos: Some(target),
            },
        );
        assert!(ok, "a valid aim must be accepted for deferral: {reason:?}");
        let state = ship.get_state();
        let gun_state = state.station_states.get(&gun).expect("seat exists");
        let yaw_ceiling = ship
            .get_station(gun)
            .expect("the tier has a gun station")
            .config()
            .yaw_range
            .1;
        assert!(
            (gun_state.target_yaw - yaw_ceiling).abs() < 1e-5,
            "the aim must be clamped into the turret's own range, got {}",
            gun_state.target_yaw
        );
        assert_eq!(
            pending.len(),
            1,
            "an unreached aim must book exactly one deferred shot"
        );
        assert_eq!(
            gun_state.ammo_count, ammo_before,
            "no round may leave the muzzle before the turret arrives"
        );
        assert!(
            sim.get_active_projectiles().is_empty(),
            "a deferred shot must not spawn anything in the same tick"
        );
    }

    /// A target the turret already points at fires now — the deferral must
    /// not turn the standing-in-range case into a wait for nothing.
    #[test]
    fn a_fire_command_at_an_already_aimed_target_fires_immediately() {
        let ship = ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        let gun = station_of(&ship, StationType::Gun);
        assert!(ship.occupy_station(gun, player));

        let muzzle_local = ship
            .get_station(gun)
            .expect("the tier has a gun station")
            .local_transform()
            .position;
        let aim_local = muzzle_local + Vec3f::new(0.0, 0.0, 100.0);
        let target = ship.local_to_world(aim_local);

        let mut pending = Vec::new();
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            player,
            &mut pending,
            &ServerCommand::FireWeapon {
                station: gun,
                target_pos: Some(target),
            },
        );
        assert!(ok, "the standing-in-range shot must fire: {reason:?}");
        assert_eq!(
            pending.len(),
            0,
            "an already-reached aim must not book a deferred shot"
        );
        assert_eq!(
            sim.get_active_projectiles().len(),
            1,
            "the immediate shot must spawn its shell in the same tick"
        );
    }

    /// Bug №664 (remainder): a far target aims ballistically.
    ///
    /// At 2500 m a 400 m/s cannonball promises 191 m of parabolic drop, so the
    /// fire-control solution pitches the barrel UP to make the shell cross the
    /// target line — the straight aim (№154) would fly flat and land 191 m
    /// short. The short-range tests above (100 m, 0.31 m of drop) stay on the
    /// straight aim exactly as they were; only ranges where the two answers
    /// visibly part company get the arc.
    #[test]
    fn a_far_target_aims_ballistically() {
        let ship = ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        let gun = station_of(&ship, StationType::Gun);
        assert!(ship.occupy_station(gun, player));

        let muzzle_local = ship
            .get_station(gun)
            .expect("the tier has a gun station")
            .local_transform()
            .position;
        // 2500 m dead ahead in ship-local terms.
        let aim_local = muzzle_local + Vec3f::new(0.0, 0.0, 2500.0);
        let target = ship.local_to_world(aim_local);

        let mut pending = Vec::new();
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            player,
            &mut pending,
            &ServerCommand::FireWeapon {
                station: gun,
                target_pos: Some(target),
            },
        );
        assert!(ok, "the far aim must be accepted: {reason:?}");

        let state = ship.get_state();
        let gun_state = state.station_states.get(&gun).expect("seat exists");
        assert!(
            (gun_state.target_yaw - 0.0).abs() < 1e-4,
            "the arc must not move the aim off the bearing, got yaw {}",
            gun_state.target_yaw
        );
        assert!(
            gun_state.target_pitch > 0.05,
            "a far target must pitch the barrel for the arc, got pitch {} (the straight answer is 0)",
            gun_state.target_pitch
        );
        assert!(
            gun_state.target_pitch < 0.8,
            "the arc pitch must stay inside a sane gun elevation, got {}",
            gun_state.target_pitch
        );
        assert_eq!(
            pending.len(),
            1,
            "a raised aim the turret has not reached yet is deferred, as before"
        );
    }

    /// A pump runs only from a pump seat, not from any other engineering seat.
    #[test]
    fn only_a_pump_station_runs_a_pump() {
        let ship = ship();
        let pump = station_of(&ship, StationType::Pump);

        let (ok, reason) = seated_command(
            &ship,
            StationType::Pump,
            &ServerCommand::ActivatePump { pump },
        );
        assert!(ok, "the pump seat lost its own pump: {reason:?}");

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::ActivatePump { pump },
        );
        assert!(!ok, "damage control ran a pump");
        assert_eq!(reason.as_deref(), Some("not at a pump station"));
    }

    /// A seat at 10 / max_health health, so any repair — honest or infinite —
    /// is visible as a number that moved.
    fn damaged_ship() -> (Ship, EntityId) {
        let ship = ship();
        let station = station_of(&ship, StationType::DamageControl);
        let mut state = ship.get_state();
        state
            .station_states
            .get_mut(&station)
            .expect("seat exists")
            .health = 10.0;
        ship.apply_state(state);
        (ship, station)
    }

    /// Bug №604 (3.1.3): `f32::INFINITY` arrived as `interaction.value`, sailed
    /// through `.max(0.0)`, and `.min(max_health)` handed out a full repair for
    /// one packet. The refusal shows up twice: as the returned reason, and as
    /// health that did not move.
    #[test]
    fn an_infinite_repair_amount_is_refused_and_fills_nothing() {
        let (ship, station) = damaged_ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        assert!(ship.occupy_station(station, player));

        let reason = handle_station_interaction(
            &sim,
            &ship,
            player,
            &StationInteraction {
                station_id: station,
                action: StationAction::Repair,
                value: f32::INFINITY,
            },
        );

        assert_eq!(
            reason.as_deref(),
            Some("repair amount inf is not finite"),
            "the infinite repair was not refused"
        );
        assert_eq!(
            ship.get_state().station_states[&station].health,
            10.0,
            "the infinite repair still moved health"
        );
    }

    /// A finite-but-absurd value is clamped to the one ceiling both paths use.
    #[test]
    fn a_huge_finite_repair_amount_is_clamped_to_the_ceiling() {
        let (ship, station) = damaged_ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        assert!(ship.occupy_station(station, player));

        let reason = handle_station_interaction(
            &sim,
            &ship,
            player,
            &StationInteraction {
                station_id: station,
                action: StationAction::Repair,
                value: 1.0e9,
            },
        );

        assert_eq!(reason, None, "a finite repair was refused");
        assert_eq!(
            ship.get_state().station_states[&station].health,
            10.0 + MAX_REPAIR_PER_ACTION,
            "the value was not clamped to MAX_REPAIR_PER_ACTION"
        );
    }

    fn some_compartment(ship: &Ship) -> EntityId {
        *ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .expect("the default frigate has compartments")
    }

    /// Bug №607: `extinguish_compartment` returned `true` for any existing
    /// compartment, so a damage-control seat acked a successful extinguishing
    /// on a compartment that was not on fire. The command path now names the
    /// cold case instead of claiming success.
    #[test]
    fn an_extinguish_on_a_cold_compartment_is_refused() {
        let ship = ship();
        let compartment = some_compartment(&ship);

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::ExtinguishFire { compartment },
        );
        assert!(!ok, "a cold compartment was acked as extinguished");
        assert_eq!(reason.as_deref(), Some("nothing is burning there"));
    }

    /// The other half of the same test: a real fire must still go out. Without
    /// this, refusing everything would also be green.
    #[test]
    fn extinguishing_puts_out_a_burning_compartment() {
        let ship = ship();
        let compartment = some_compartment(&ship);
        assert!(
            ship.ignite_compartment(compartment, 0.5),
            "the fixture could not light the compartment"
        );

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::ExtinguishFire { compartment },
        );
        assert!(ok, "damage control could not put out a fire: {reason:?}");
        assert_eq!(
            ship.get_state().compartment_states[&compartment].fire_intensity,
            0.0,
            "the fire was acked but not put out"
        );
    }

    /// The continuous input path carries the same truth, not the old silent
    /// success.
    #[test]
    fn the_input_path_also_refuses_a_cold_extinguish() {
        let ship = ship();
        let sim = TickSystem::new();
        let player = EntityId::new(42);
        let station = station_of(&ship, StationType::DamageControl);
        assert!(ship.occupy_station(station, player));

        let reason = handle_station_interaction(
            &sim,
            &ship,
            player,
            &StationInteraction {
                station_id: station,
                action: StationAction::ExtinguishFire,
                value: 0.0,
            },
        );

        assert_eq!(reason.as_deref(), Some("nothing is burning there"));
    }

    /// Bug №669: the occupant of a station can flip its default behavior.
    /// The wire carries small codes (0 = Active, 1 = Passive, 2 = Disabled);
    /// the seat check follows the same `commands_station` rule as firing.
    #[test]
    fn the_occupant_can_set_a_station_behavior() {
        let ship = ship();
        let station = station_of(&ship, StationType::Gun);
        let fixture_player = EntityId::new(42);
        assert!(
            ship.occupy_station(station, fixture_player),
            "fixture could not sit"
        );

        let sim = TickSystem::new();
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            fixture_player,
            &mut Vec::new(),
            &ServerCommand::SetStationBehavior {
                station,
                behavior: 2,
            },
        );
        assert!(ok, "the occupant could not disable the station: {reason:?}");
        assert!(
            !ship.get_state().station_states[&station].is_operational,
            "the Disabled code left the station operational"
        );

        // And back on: the same seat, code 0.
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            fixture_player,
            &mut Vec::new(),
            &ServerCommand::SetStationBehavior {
                station,
                behavior: 0,
            },
        );
        assert!(
            ok,
            "the occupant could not reactivate the station: {reason:?}"
        );
        assert!(
            ship.get_state().station_states[&station].is_operational,
            "the Active code left the station down"
        );
    }

    /// An unknown behavior code is refused in the ack, not silently mapped
    /// onto an existing behavior: a typo visible as a refusal is a contract
    /// both sides can debug.
    #[test]
    fn an_unknown_behavior_code_is_refused() {
        let ship = ship();
        let station = station_of(&ship, StationType::Gun);
        let (ok, reason) = seated_command(
            &ship,
            StationType::Gun,
            &ServerCommand::SetStationBehavior {
                station,
                behavior: 77,
            },
        );
        assert!(!ok, "an unknown code was silently accepted");
        assert_eq!(reason.as_deref(), Some("unknown behavior code 77"));
        assert!(
            ship.get_state().station_states[&station].is_operational,
            "a refused code still reached the station"
        );
    }

    /// The seat rule holds for behavior as well: an unseated player cannot
    /// flip somebody else's gun.
    #[test]
    fn a_player_at_no_station_cannot_set_behavior() {
        let ship = ship();
        let station = station_of(&ship, StationType::Gun);
        let sim = TickSystem::new();
        let (ok, reason) = translate_command(
            &sim,
            &ship,
            EntityId::new(42),
            &mut Vec::new(),
            &ServerCommand::SetStationBehavior {
                station,
                behavior: 2,
            },
        );
        assert!(!ok, "an unseated player flipped a station");
        assert_eq!(reason.as_deref(), Some("station not occupied by you"));
    }

    /// Bug №668: the compartment-level seal got its own command alongside the
    /// bulkhead one, and lives by the same seat rule: damage control only.
    #[test]
    fn the_damage_control_seat_can_seal_a_compartment() {
        let ship = ship();
        let compartment = ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .copied()
            .expect("the default frigate has compartments");

        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::SetCompartmentSeal {
                compartment,
                sealed: true,
            },
        );
        assert!(
            ok,
            "damage control could not seal the compartment: {reason:?}"
        );
        assert!(ship.get_state().compartment_states[&compartment].is_sealed);

        // And back: the same seat, open side.
        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::SetCompartmentSeal {
                compartment,
                sealed: false,
            },
        );
        assert!(
            ok,
            "damage control could not open the compartment: {reason:?}"
        );
        assert!(!ship.get_state().compartment_states[&compartment].is_sealed);
    }

    /// The seat rule holds for the compartment seal too, and the refusal must
    /// leave the flag untouched.
    #[test]
    fn the_helmsman_cannot_seal_a_compartment() {
        let ship = ship();
        let compartment = ship
            .get_state()
            .compartment_states
            .keys()
            .next()
            .copied()
            .expect("the default frigate has compartments");
        let (ok, reason) = seated_command(
            &ship,
            StationType::Helm,
            &ServerCommand::SetCompartmentSeal {
                compartment,
                sealed: false,
            },
        );
        assert!(!ok, "the helmsman sealed a compartment");
        assert_eq!(reason.as_deref(), Some("not at a damage-control station"));
        assert!(
            ship.get_state().compartment_states[&compartment].is_sealed,
            "a refused seal command still moved the flag"
        );
    }

    /// An unknown compartment is a refusal, not a silent ack: a typo has to
    /// be visible on the wire.
    #[test]
    fn an_unknown_compartment_is_refused_by_the_seal_command() {
        let ship = ship();
        let (ok, reason) = seated_command(
            &ship,
            StationType::DamageControl,
            &ServerCommand::SetCompartmentSeal {
                compartment: EntityId::new(999_999),
                sealed: true,
            },
        );
        assert!(!ok, "an unknown compartment was acked");
        assert_eq!(reason.as_deref(), Some("compartment not found"));
    }
}
