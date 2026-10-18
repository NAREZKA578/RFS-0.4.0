//! Every `.rs` file in `src/` must be reachable from the module tree.
//!
//! # Why this test exists
//!
//! A subsystem can be written, polished, covered by its own unit tests, and still
//! never run — because nothing declares it. Nothing about that state fails a
//! build: the file simply sits on disk, counting towards the project's line
//! totals and appearing in every "what does this project have" summary.
//!
//! It has already happened twice here, and the second time is the embarrassing
//! one. The entire `rhi::ui` module — text layout, input handling, two pipelines
//! — had no `pub mod ui;` in `lib.rs`. It had never been compiled. Its 140 tests
//! were "passing" in a build where the module was not part of the crate; the count
//! was 59. Every claim that the UI pipelines were unit-tested was therefore
//! false, and the reason nobody noticed is precisely this test's subject: nothing
//! connects "code exists" to "code runs".
//!
//! Then, immediately after connecting the module, `ui/theme.rs` turned out to be
//! unconnected as well — the same defect, one level down, missed by the fix that
//! was made in that very area.
//!
//! So this walks the tree and compares it against the module declarations. It is
//! the cheapest possible guard against the most expensive possible mistake, and
//! it costs one `cargo test`.
//!
//! # Allowlist
//!
//! Files that are genuinely not part of the tree must be listed below with a
//! reason. An allowlist entry without a reason is treated as a failure of this
//! test, because "it was always like that" is not a reason.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

/// A file that is deliberately not reachable from the module tree.
///
/// Each one is a promise to revisit. Keep the reason honest and specific: a
/// vague reason here becomes an invisible file in a year.
struct AllowedDead {
    path: &'static str,
    reason: &'static str,
}

/// Files currently outside the module tree.
///
/// Every entry is real, found by walking `src/`, not imagined.
const ALLOWED_DEAD: &[AllowedDead] = &[
    AllowedDead {
        path: "src/rhi/prelude.rs",
        reason: "References `crate::rhi::Rhi`, a module that does not exist, so it \
                 cannot compile even if declared. Delete it rather than connect it.",
    },
    AllowedDead {
        path: "src/rhi/backend/d3d11/device.rs",
        reason: "D3D11 backend stub. `Cargo.toml` advertises D3D11 as supported; the \
                 implementation is not ported. Either port it or drop the feature.",
    },
    AllowedDead {
        path: "src/rhi/backend/d3d12/device.rs",
        reason: "D3D12 backend stub, same as D3D11. `Cargo.toml` advertises it as \
                 supported and nothing implements it.",
    },
    AllowedDead {
        path: "src/rhi/backend/opengl/device.rs",
        reason: "OpenGL backend stub, same as D3D11/D3D12.",
    },
    AllowedDead {
        path: "src/rhi/backend/vulkan/pipeline_cache.rs",
        reason: "Both pipeline creation sites pass `vk::PipelineCache::null()`, so this \
                 is 76 lines that never execute. Every start-up recompiles every \
                 shader from SPIR-V to ISA.",
    },
];

fn repo_root() -> PathBuf {
    // CARGO_MANIFEST_DIR is the `rfs-client` package root, which is the repo root.
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// Every `.rs` under `src/`, relative to the repo root, sorted.
fn source_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_rust_files(&root.join("src"), root, &mut out);
    out.sort();
    out
}

fn collect_rust_files(dir: &Path, root: &Path, out: &mut Vec<String>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rust_files(&path, root, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            if let Ok(rel) = path.strip_prefix(root) {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
}

/// The `mod` stems declared by the parent directory, if it declares any.
fn declared_stems(dir: &Path) -> Option<Vec<String>> {
    let mod_rs = dir.join("mod.rs");
    let lib_rs = dir.join("lib.rs");
    let source = if mod_rs.exists() {
        fs::read_to_string(&mod_rs).ok()
    } else if lib_rs.exists() {
        fs::read_to_string(&lib_rs).ok()
    } else {
        None
    }?;

    let mut stems = Vec::new();
    for line in source.lines() {
        let line = line.trim();
        // `mod foo;` and `pub mod foo;`, ignoring anything with a body (`mod foo {`).
        let rest = line.strip_prefix("mod ").or_else(|| line.strip_prefix("pub mod "));
        if let Some(rest) = rest {
            let rest = rest.trim();
            if rest.ends_with(';') {
                let stem: String = rest
                    .trim_end_matches(';')
                    .trim()
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                if !stem.is_empty() {
                    stems.push(stem);
                }
            }
        }
    }
    Some(stems)
}

/// Walks the tree and reports every source file the module tree cannot reach.
#[test]
fn every_source_file_is_reachable_from_the_module_tree() {
    let root = repo_root();
    let files = source_files(&root);
    assert!(
        !files.is_empty(),
        "found no source files under {} — the walk itself is broken, not the tree",
        root.join("src").display()
    );

    let mut by_dir: BTreeMap<PathBuf, Vec<String>> = BTreeMap::new();
    for file in &files {
        let path = Path::new(file);
        if let Some(parent) = path.parent() {
            by_dir.entry(parent.to_path_buf()).or_default().push(
                path.file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }

    // Crate roots are declared by Cargo, not by a parent `mod`.
    const CRATE_ROOTS: &[&str] = &["src/rhi", "src/render", "src"];
    let allowed: BTreeMap<&str, &str> = ALLOWED_DEAD
        .iter()
        .map(|a| (a.path, a.reason))
        .collect();

    let mut unconnected: Vec<String> = Vec::new();
    let mut stale_allowlist: Vec<String> = Vec::new();

    for (dir, stems) in &by_dir {
        if CRATE_ROOTS.contains(&dir.to_string_lossy().replace('\\', "/").as_str()) {
            continue;
        }
        let Some(declared) = declared_stems(dir) else {
            continue;
        };
        for stem in stems {
            // `mod.rs` declares its children and is itself declared by the parent
            // as `mod foo;`; `lib.rs`/`main.rs` are the crate roots declared by
            // Cargo. None of the three appear as a `mod` item in their own parent.
            if stem == "mod" || stem == "lib" || stem == "main" {
                continue;
            }
            if declared.iter().any(|d| d == stem) {
                continue;
            }
            let rel = dir.join(format!("{stem}.rs"));
            let rel = rel.to_string_lossy().replace('\\', "/");
            if allowed.contains_key(rel.as_str()) {
                continue;
            }
            unconnected.push(rel);
        }
    }

    // An allowlist entry pointing at a file that is now connected, or gone,
    // means the entry itself has rotted.
    for (path, reason) in &allowed {
        if !files.iter().any(|f| f == path) {
            stale_allowlist.push(format!(
                "{path} is allowlisted but no longer exists — drop the entry.\n     \
                 reason on record: {reason}"
            ));
        } else {
            let stem = Path::new(path).file_stem().unwrap().to_string_lossy().to_string();
            if let Some(dir) = Path::new(path).parent() {
                if let Some(declared) = declared_stems(dir) {
                    if declared.iter().any(|d| *d == stem) {
                        stale_allowlist.push(format!(
                            "{path} is allowlisted as dead but is now connected — \
                             drop the entry, it is no longer dead."
                        ));
                    }
                }
            }
        }
    }

    assert!(
        stale_allowlist.is_empty(),
        "the allowlist in this test has rotted:\n  - {}",
        stale_allowlist.join("\n  - ")
    );

    assert!(
        unconnected.is_empty(),
        "these files exist but nothing declares them, so they are never compiled \
         and never run. This is how the whole `rhi::ui` module went missing from \
         the build while its tests were reported as passing.\n  - {}\n\n\
         Fix: declare the module, or add it to ALLOWED_DEAD in this file with a \
         reason.",
        unconnected.join("\n  - ")
    );
}

/// The allowlist must not become a dumping ground.
///
/// If most of the tree is excused, the test stops meaning anything, and the next
/// silent disappearance lands unimpeded.
#[test]
fn the_allowlist_stays_a_small_minority() {
    let total = source_files(&repo_root()).len();
    let excused = ALLOWED_DEAD.len();
    assert!(
        total > 0 && excused * 5 <= total,
        "{} of {total} source files are allowlisted as dead. That is too much of \
         the tree to be excusing; either connect them or delete them.",
        excused
    );
}
