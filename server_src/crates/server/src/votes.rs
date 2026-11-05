//! The VoteKick machine — the pure counting state; the glue (acks, the kick
//! itself) is in `handle_vote_kick` of the same file.
//!
//! Owner decision (2026-10-19): the feature stays, its strength is
//! config-driven — enabled flag, vote floor, share of online players, window.
//! The machine itself does not know about sockets, ships or time beyond the
//! tick counters its owner passes in.

use rfs_core::entity::EntityId;
use rfs_core::packet::{CommandPacket, DisconnectReason, ServerCommand};
use rfs_net::NetServer;
use rfs_sim::tick::TickSystem;
use std::collections::{HashMap, HashSet};
use tracing::info;

use crate::config::VoteKickConfig;
use crate::match_::{despawn_player, PlayerMeta};

/// One running vote. There is at most one at a time: voting against where the
/// quorum already stands is social pressure, not bookkeeping — a new
/// proposal waits for the old one to resolve.
#[derive(Debug, Clone, PartialEq)]
struct ActiveVote {
    target: EntityId,
    votes: HashSet<EntityId>,
    /// Simulation ticks until expiry. Re-armed on every ballot: a vote lives
    /// as long as it keeps gathering interest, and dies when nobody caring is
    /// left.
    ttl: u64,
}

/// What one ballot produced; every variant maps to an honest ack reason.
#[derive(Debug, Clone, PartialEq)]
pub enum VoteOutcome {
    /// The vote opened; `votes_left` is how many more are needed.
    Opened { votes_left: u32 },
    /// The ballot was counted; `votes_left` is how many more are needed.
    Counted { votes_left: u32 },
    /// This player already voted; the ballot changed nothing.
    AlreadyVoted,
    /// Enough votes; the *caller* performs the kick.
    Passed,
    /// A vote against another player is running.
    OtherVoteRunning { target: EntityId },
}

pub struct VoteKickState {
    config: VoteKickConfig,
    active: Option<ActiveVote>,
}

impl VoteKickState {
    pub fn new(config: VoteKickConfig) -> Self {
        Self {
            config,
            active: None,
        }
    }

    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// Stage 2 `reload`: replaces the counting rules from the next ballot.
    /// An already-open vote keeps its target and its tallies — the ballots
    /// already cast were counted against the rules their voters saw, and
    /// re-deciding a verdict in flight is how a quorum turns into a lie. The
    /// re-armed window (`window_ticks`) also applies to the *next* ballot,
    /// not retroactively to the one standing.
    pub fn apply_config(&mut self, config: VoteKickConfig) {
        self.config = config;
    }

    /// The vote floor and share: `max(min_votes, ceil(majority * online))`.
    fn votes_needed(&self, online: usize) -> u32 {
        let by_share = (self.config.majority * online.max(1) as f32).ceil();
        self.config.min_votes.max(by_share as u32).max(1)
    }

    pub fn cast(&mut self, voter: EntityId, target: EntityId, online: usize) -> VoteOutcome {
        let need = self.votes_needed(online);

        if let Some(active) = &mut self.active {
            if active.target != target {
                return VoteOutcome::OtherVoteRunning {
                    target: active.target,
                };
            }
            if !active.votes.insert(voter) {
                return VoteOutcome::AlreadyVoted;
            }
            // Every ballot re-arms the window: a vote that keeps gathering
            // interest lives on, and a slow but real quorum is reachable.
            active.ttl = self.config.window_ticks;
        } else {
            let mut votes = HashSet::new();
            votes.insert(voter);
            self.active = Some(ActiveVote {
                target,
                votes,
                ttl: self.config.window_ticks,
            });
        }

        let got = self.active.as_ref().expect("the vote exists").votes.len() as u32;
        if got >= need {
            // The verdict closes the vote: nothing is left to count, and a
            // quorum that passed must not keep forbidding future proposals.
            self.active = None;
            return VoteOutcome::Passed;
        }
        let votes_left = need - got;
        if got == 1 {
            VoteOutcome::Opened { votes_left }
        } else {
            VoteOutcome::Counted { votes_left }
        }
    }

    /// One resolve pass per simulation tick. Returns the target of a vote
    /// that has just expired; nothing needs to happen — but the caller logs
    /// it, because an expired vote must not look like a vote that is still
    /// open.
    pub fn resolve(&mut self) -> Option<EntityId> {
        let Some(active) = &mut self.active else {
            return None;
        };
        if active.ttl <= 1 {
            let target = active.target;
            self.active = None;
            return Some(target);
        }
        active.ttl -= 1;
        None
    }
}

/// The glue between the vote machine and the server: the ack the ballot
/// deserves and, when the quorum stands, the kick itself.
///
/// Lives beside `handle_command`'s level of abstraction, but reads the
/// server-wide pieces (net, the players map) that the per-ship command
/// translator does not see, which is why `ServerCommand::VoteKick` is routed
/// here directly in the drain instead of through `translate_command`.
pub fn handle_vote_kick(
    net: &NetServer,
    sim: &TickSystem,
    connection_id: u32,
    player_id: EntityId,
    command: &CommandPacket,
    votes: &mut VoteKickState,
    players: &mut HashMap<u32, PlayerMeta>,
) {
    let acknowledge = |success: bool, reason: Option<String>| {
        net.send_command_ack(connection_id, command.command_id, success, reason);
    };

    let ServerCommand::VoteKick {
        initiator: _,
        target,
    } = &command.command
    else {
        acknowledge(
            false,
            Some("vote-kick glue received a non-vote command".into()),
        );
        return;
    };

    if !votes.enabled() {
        acknowledge(
            false,
            Some("vote-kick is disabled by the server config".into()),
        );
        return;
    }

    // Auth: the client's `initiator` field is spoofable, the same rule as
    // ChatMessage's `player` — the vote belongs to the connection's
    // authenticated player, arrived by whichever name the client claims.
    let Some(target_connection) = players
        .iter()
        .find(|(_, meta)| meta.player_entity_id == *target)
        .map(|(connection, _)| *connection)
    else {
        acknowledge(false, Some("vote target is not online".into()));
        return;
    };
    let online = players.len();

    match votes.cast(player_id, *target, online) {
        VoteOutcome::Opened { votes_left } => {
            acknowledge(
                true,
                Some(format!("vote opened: {votes_left} more vote(s) needed")),
            );
        }
        VoteOutcome::Counted { votes_left } => {
            acknowledge(
                true,
                Some(format!("vote counted: {votes_left} more vote(s) needed")),
            );
        }
        VoteOutcome::AlreadyVoted => {
            acknowledge(false, Some("you already voted".into()));
        }
        VoteOutcome::OtherVoteRunning { target: running } => {
            acknowledge(
                false,
                Some(format!(
                    "a vote against player {:?} is already running",
                    running.0
                )),
            );
        }
        VoteOutcome::Passed => {
            // Order matters: the connection goes first — no more inputs can
            // arrive from a client that is already gone — and the despawn
            // clears the interest entry, vacates stations and broadcasts.
            // A kick against a player who left between ballots lands nowhere:
            // the verdict is real, the target is not.
            if net.disconnect_client(target_connection, DisconnectReason::Kicked) {
                info!("vote-kick passed; disconnecting connection {target_connection}");
                despawn_player(sim, net, target_connection, players);
            }
            acknowledge(true, Some("kick passed".into()));
        }
    }
}

#[cfg(test)]
mod vote_kick_tests {
    use super::*;

    fn config() -> VoteKickConfig {
        VoteKickConfig {
            enabled: true,
            min_votes: 2,
            majority: 0.6,
            window_ticks: 10,
        }
    }

    fn p(n: u64) -> EntityId {
        EntityId::new(n)
    }

    /// Three online, share 0.6 → ceil(1.8) = 2, floor 2 → two ballots pass.
    /// The first ballot opens the vote and counts as one.
    #[test]
    fn the_first_ballot_is_the_opening_vote() {
        let mut state = VoteKickState::new(config());
        let voter = p(1);
        let target = p(2);

        assert_eq!(
            state.cast(voter, target, 3),
            VoteOutcome::Opened { votes_left: 1 }
        );
        // The second ballot from the same player is not a second vote.
        assert_eq!(state.cast(voter, target, 3), VoteOutcome::AlreadyVoted);
        assert_eq!(
            state.cast(p(3), target, 3),
            VoteOutcome::Passed,
            "two of three players is the configured share"
        );

        // The verdict closes the vote: the next proposal starts clean
        // instead of waiting behind a vote that already reached its quorum.
        assert_eq!(
            state.cast(p(3), p(5), 3),
            VoteOutcome::Opened { votes_left: 1 },
            "a passed vote must not keep forbidding proposals"
        );
    }

    /// The floor exists so a sole player cannot pass a vote against anything,
    /// even against themselves: one ballot opens, the same player's repeat is
    /// not a second vote, and the window expires the lone attempt.
    #[test]
    fn the_floor_beats_the_share_when_the_server_is_empty() {
        let mut state = VoteKickState::new(config());
        let target = p(2);

        // One online, share gives ceil(0.6) = 1, but the floor is 2.
        assert_eq!(
            state.cast(target, target, 1),
            VoteOutcome::Opened { votes_left: 1 }
        );
        assert_eq!(
            state.cast(target, target, 1),
            VoteOutcome::AlreadyVoted,
            "one player cannot supply the second vote to kick anybody, self included"
        );
        for _ in 0..9 {
            state.resolve();
        }
        assert_eq!(state.resolve(), Some(target), "the lone attempt expires");
        assert!(
            matches!(
                state.cast(p(7), p(8), 1),
                VoteOutcome::Opened { votes_left: 1 }
            ),
            "after expiry everything starts from zero"
        );
    }

    #[test]
    fn a_second_target_waits_for_the_first_vote_to_finish() {
        let mut state = VoteKickState::new(config());
        state.cast(p(1), p(2), 3);

        assert_eq!(
            state.cast(p(3), p(5), 3),
            VoteOutcome::OtherVoteRunning { target: p(2) },
            "one vote at a time; a second target waits"
        );
    }

    /// The window re-arms on every ballot: a vote gathering real interest
    /// lives on, one nobody follows dies.
    #[test]
    fn a_vote_with_no_followers_expires() {
        let mut state = VoteKickState::new(config());
        let target = p(2);
        state.cast(p(1), target, 3);

        for _ in 0..9 {
            assert!(state.resolve().is_none(), "the window is 10 ticks");
        }
        assert_eq!(state.resolve(), Some(target), "tick 10 expires the vote");

        // After expiry a fresh vote opens against the same or another
        // target; the dead vote holds nothing and forbids nothing.
        assert_eq!(
            state.cast(p(3), p(5), 3),
            VoteOutcome::Opened { votes_left: 1 }
        );
    }

    /// Each ballot re-arms the window, so a slow but interested vote can
    /// actually reach its quorum. The re-arm is visible only after the
    /// outcome differences run out: nine ticks after the *last* ballot the
    /// vote must still be standing to reject a compete target.
    #[test]
    fn a_ballot_re_arms_the_window() {
        let mut state = VoteKickState::new(config());
        let target = p(2);
        state.cast(p(1), target, 10);
        state.resolve();
        // Second ballot: 10 online, share 0.6 → need 6 → the vote stands at 2.
        assert_eq!(
            state.cast(p(3), target, 10),
            VoteOutcome::Counted { votes_left: 4 }
        );

        for _ in 0..9 {
            state.resolve();
        }
        assert_eq!(
            state.cast(p(1), p(5), 10),
            VoteOutcome::OtherVoteRunning { target },
            "nine ticks after the ballot the re-armed vote must still be running"
        );
    }

    /// Stage 2 `reload`: the counting rules change from the next ballot. An
    /// open vote keeps its target and its tallies — the ballots in flight were
    /// counted against the strength their voters saw, and wiping or passing
    /// them on apply would both rewrite a verdict. The next ballot recounts
    /// against the new strength (the same live-need rule every cast uses), so
    /// it neither passes nor re-opens the vote it joined.
    #[test]
    fn apply_config_keeps_an_open_vote_and_re_rules_the_next_ballot() {
        let mut state = VoteKickState::new(config());
        let target = p(2);

        // Five online, old rules: need = max(2, ceil(3)) = 3; two ballots stand.
        assert_eq!(
            state.cast(p(1), target, 5),
            VoteOutcome::Opened { votes_left: 2 }
        );
        assert_eq!(
            state.cast(p(3), target, 5),
            VoteOutcome::Counted { votes_left: 1 }
        );

        // The reloaded version demands unanimity and the floor of one.
        state.apply_config(VoteKickConfig {
            enabled: true,
            min_votes: 1,
            majority: 1.0,
            window_ticks: 10,
        });

        // The applied config must not close a standing vote...
        assert!(
            state.resolve().is_none(),
            "an applied config must not close the vote it found underway"
        );
        // ...and its third ballot joins the STANDING vote, counted against the
        // NEW need (max(1, ceil(5)) = 5): three ballots stand, one short. The
        // no-op probe would pass it at the old quorum of 3; a tally-wipe
        // probe would re-open at 1.
        assert_eq!(
            state.cast(p(11), target, 5),
            VoteOutcome::Counted { votes_left: 2 },
            "tallies must survive the reload and count under the new strength"
        );
    }
}
