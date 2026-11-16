//! Bug №798: the two `[workspace.dependencies]` tables mirror each other on
//! purpose ("keep the two lists in lockstep", root `Cargo.toml`). Cargo
//! resolves the `workspace = true` inheritance of the path dependencies
//! that live inside this directory tree (`rfs_core`, `rfs_net`, pulled by
//! the menu's Play) against THIS manifest, while the same crates built
//! from the server workspace resolve against `server_src/Cargo.toml`.
//! A version changed in one place but not the other makes the same crate
//! build against different dependency versions depending on which
//! workspace pulled it — the drift this test refuses.
//!
//! The comparison is spec-level: for every dependency declared in BOTH
//! tables, the spec strings must be equal after whitespace stripping.
//! The one documented deviation is `tokio` (root manifest: the client
//! carries only the drivers it needs instead of the server's `full`);
//! for it the versions must still match.

use std::collections::BTreeMap;

/// Reads the `[workspace.dependencies]` table of a manifest into
/// `name -> spec-as-written` (comments and blanks skipped). A second
/// `[section]` header ends the table.
fn workspace_dependencies_table(manifest: &str) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let mut inside = false;
    for line in manifest.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            inside = trimmed == "[workspace.dependencies]";
            continue;
        }
        if !inside || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let (name, spec) = trimmed
            .split_once('=')
            .unwrap_or_else(|| panic!("a dependency line without '=': {trimmed}"));
        map.insert(name.trim().to_string(), spec.trim().to_string());
    }
    map
}

/// The spec with all whitespace removed, so `a = { version = "1", ... }`
/// and `a = {version="1",...}` compare equal.
fn normalized(spec: &str) -> String {
    spec.chars().filter(|c| !c.is_whitespace()).collect()
}

/// The version a spec pins: the string itself for the plain `"1.38"` form,
/// the `version = "..."` field for the table form.
fn version_of(spec: &str) -> String {
    let packed = normalized(spec);
    if !packed.starts_with('{') {
        return packed.trim_matches('"').to_string();
    }
    let body = packed.trim_start_matches('{').trim_end_matches('}');
    for part in body.split(',') {
        let part = part.trim();
        if let Some(rest) = part.strip_prefix("version") {
            return rest.trim_start_matches('=').trim().trim_matches('"').to_string();
        }
    }
    panic!("a workspace dependency without a version: {packed}");
}

#[test]
fn the_two_workspace_dependency_lists_stay_in_lockstep() {
    let root = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"))
        .expect("the root manifest is readable");
    let server =
        std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/server_src/Cargo.toml"))
            .expect("the server manifest is readable");

    let client = workspace_dependencies_table(&root);
    let server = workspace_dependencies_table(&server);

    // The mirror's subject set is real; if either side restructures its
    // table, this anchor names what moved.
    assert!(
        client.contains_key("tokio") && server.contains_key("tokio"),
        "tokio left one of the [workspace.dependencies] tables; re-point this test"
    );

    let shared: Vec<&String> = client
        .keys()
        .filter(|name| server.contains_key(name.as_str()))
        .collect();
    assert!(
        shared.len() >= 10,
        "the shared dependency set collapsed: {shared:?}"
    );

    for name in shared {
        let client_spec = &client[name.as_str()];
        let server_spec = &server[name.as_str()];
        if name == "tokio" {
            // Documented deviation: features differ by design (client
            // carries the drivers it needs, the server uses `full`); the
            // version must not drift.
            assert_eq!(
                version_of(client_spec),
                version_of(server_spec),
                "tokio version drifted between the two workspaces"
            );
        } else {
            assert_eq!(
                normalized(client_spec),
                normalized(server_spec),
                "dependency {name} drifted between the two [workspace.dependencies] tables"
            );
        }
    }
}
