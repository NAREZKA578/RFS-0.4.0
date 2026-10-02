# Registry audit — what is actually left to fix

Written 2026-10-01. Everything here is a measured result, not a recollection.

## Why this file exists

`BUGS.md` cannot be trusted as a work list. Entries whose fix already landed in
the code are still marked `не исправлен`, and they point at line numbers that
moved when the code was refactored. Fixing straight from the list would have
meant editing working code to match a stale description.

## Results (2026-10-01)

Of 121 entries with status `не исправлен`:

| Group | Count | Meaning |
|---|---|---|
| No `Bug №N` marker in the referenced file | 101 | probably genuinely open |
| Marker present in the code | 13 | registry is very likely stale |
| Path in the registry did not resolve | 7 | needs a human look |

**Suspected stale (13):** 30, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 74, 206

**Unresolved paths (7):** 67, 93, 121, 127, 131, 138, 187

## What was verified (2026-10-01)

All 13 suspected stale entries were read and confirmed fixed:

- **№58** — despawn_player now calls vacate_station (main.rs:288-314).
  occupy_station no longer sets is_operational=true (ship.rs:596-621).
  vacate_station no longer sets is_operational=false (ship.rs:657-667).
- **№59** — steering arbitrated via current_helm_occupant (main.rs:335-354).
  Station ownership checked in handle_station_interaction (main.rs:356-367).
  translate_command validates occupant (main.rs:516-529).
- **№60** — documented in comment (packet.rs:12-36). Declaration order pinned
  by test (Tool/harness/tests/protocol.rs). Vulnerability remains: reorder
  silently breaks the wire despite explicit `=N`.
- **№61** — StationStateData contains all fields (packet.rs:325-342).
  CompartmentState contains pump_active (packet.rs:311-323).
  StationEntity in entity.rs also contains all fields (entity.rs:134-156).
- **№62** — update_position uses new_pos (spatial.rs:205-238) with finite check.
  MAX_ENTITIES_PER_CELL enforced via spill list (spatial.rs:153-167).
  query_radius has finite checks (spatial.rs:253-261).
- **№74** — client retransmits Connect every 250 ms until Accept/Reject or
  5 s timeout (client.rs:20-23, 535-581).
- **№206** — defragment does not clear stats (pool.rs:262-270). free()
  returns Result (pool.rs:227-254) — checks unknown allocation and double free.

All 7 unresolved-path entries were read and resolved:

- **№67** — bounded channel (CLIENT_EVENT_QUEUE=4096) instead of unbounded
  (client.rs:68,357). Coalescing slot for snapshots (client.rs:253).
  EventSink with backpressure (client.rs:129-220). Bot drains events via
  try_recv (stress/main.rs:277-348).
- **№93** — bugs №77-92 closed, project compiles (cargo check passes).
  Render module now available for verification.
- **№121** — RenderContext has device() method (context.rs:233).
  effect_manager.rs uses it correctly (particles/effect_manager.rs:77).
- **№127** — conversion.rs: all three methods implemented (to_vulkan:51-135,
  to_dxgi:144-210, to_gl:216-244). alignment.rs: guard on 0 (align_up:19-54,
  is_aligned). hash.rs: hash_pipeline_desc does not return 0 (hash.rs:45-52).
- **№131** — types defined (particles/effects/mod.rs:25-43). Re-exported via
  particles/mod.rs (particles/mod.rs:12-17). render/mod.rs imports correctly
  (render/mod.rs:65-67). ParticleSystem connected to RHI (system.rs:11).
- **№138** — Vulkan implemented (vulkan/mod.rs — instance, device, memory,
  present, record, convert). D3D12/D3D11/OpenGL — stubs returning NotSupported.
  No unimplemented!() in code.
- **№187** — NOT FIXED. GPU tests still call VulkanBackend::new().unwrap()
  without checking for Vulkan availability. rhi_backend test has empty Ok
  branch.

## Two traps in the tooling

Both cost real time and will cost it again.

1. **The registry has two sections with identical field names.** One is the
   `N` bug list, the other is the `K` compilation-error list. They share field
   names and their numbering overlaps, so a naive count merges them and produces
   nonsense. The discriminator is that only the `K` entries have an error-code
   field.
2. **Cyrillic in a PowerShell command line arrives corrupted.** Every regex and
   every comparison then fails silently, and the symptom is "0 results", not an
   error. Any script touching this repository must either be written to a file
   with the `write` tool (which preserves UTF-8) or avoid Cyrillic entirely —
   key off field *lengths* and value *shape*, and build `№` from `[char]0x2116`.
   Note that PowerShell 5.1 also reads a `.ps1` file without a BOM as ANSI, so a
   saved script containing Cyrillic fails to parse.

## Also worth knowing

- One entry has a status value 24 characters long, which matches none of the
  three expected words. Likely a typo or stray text; look at it.
- Around 60 of the remaining entries are of the form "this file is a stub":
  BLAS/TLAS, timeline semaphores, D3D12/D3D11/OpenGL backends, particles, UI,
  water, post-processing. Those are not bugs to fix, they are subsystems to
  write, and no task list makes that distinction visible.

## Recommended order

1. Fix №187 (GPU tests panic in CI) — straightforward, improves test stability.
2. Fix №30 (LayerBase advanced before send) — requires ACK/retransmit mechanism.
3. Fix the ~101 real bugs in batches of three or four, each with a regression
   test, keeping `clippy=0` / `test=0` green in all three workspaces
   (`C:\RFS-0.4.0`, `C:\RFS-0.4.0\server_src`, `C:\RFS-0.4.0\Tool`).
4. Decide separately what to do about the stub subsystems.
