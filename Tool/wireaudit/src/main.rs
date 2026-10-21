//! Wire audit: find code that is written but never reached.
//!
//! # The problem
//!
//! Rust's `dead_code` lint only fires on items the crate cannot see a use for.
//! Every `pub` item in a library crate is considered used, because a downstream
//! crate might use it. This project's subsystems are `pub` almost throughout, so
//! the lint is silent about the exact class of bug that has been costing time:
//! a subsystem that is fully implemented, fully tested in isolation, and never
//! entered by anything.
//!
//! Three real examples found in one afternoon, none of which any test caught:
//!
//!  - `UiRenderer::register_font` and `set_atlas` were never called from
//!    anywhere, so the entire textured pipeline could not run. Text was silently
//!    counted as "skipped".
//!  - `CollisionSystem` had a complete `step`, a broadphase and a raycast, and
//!    `TickSystem::check_collisions` never referenced it. Nothing in the world
//!    collided with anything.
//!  - `Command::ClearColor` was implemented and translated, and produced an
//!    invalid aspect mask, and nothing in the server ever issued it.
//!
//! All three are invisible to `cargo test`, because each one's *unit* behaviour
//! is correct. What is missing is the edge.
//!
//! # What it reports, precisely
//!
//! For every public item it finds, whether any **other file** in the scanned
//! trees mentions the identifier. An item referenced only inside its own file is
//! "file-local": it may be called by its siblings, or it may be the missing edge
//! itself.
//!
//! # What it does not do, and why that matters
//!
//! It is not a call graph. It does not know whether a caller is itself reachable
//! from `main`. A method called only from another dead method is reported as
//! wired, and that is a known false negative, stated here rather than left to be
//! discovered.
//!
//! It is a scanner over text, not a compiler front end. Macro-generated code,
//! `#[cfg]`-gated branches in files it does not read, and re-export chains can
//! all produce false positives.
//!
//! Both limits are why the output is a worklist and not a verdict. The tool is
//! for finding candidates to look at, and the point is that a human confirms
//! each one - which is a few minutes of looking, against hours of guessing
//! whether a silent subsystem is a bug or a deliberate extension point.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// A public item and where it was defined.
struct Item {
    name: String,
    file: PathBuf,
    line: usize,
    kind: &'static str,
}

/// Roots scanned. Chosen to cover both engines: the client and the server.
const DEFAULT_ROOTS: &[&str] = &["../src", "../server_src/crates", "."];

fn main() {
    let mut roots: Vec<String> = std::env::args().skip(1).collect();
    if roots.is_empty() {
        roots = DEFAULT_ROOTS.iter().map(|s| s.to_string()).collect();
    }
    // A filter narrows the report to one subsystem: `wireaudit src/rhi/ui`.
    let filter = std::env::var("WIREAUDIT_FILTER").ok();

    let mut files = Vec::new();
    for root in &roots {
        collect_rs(Path::new(root), &mut files);
    }
    files.sort();
    files.dedup();
    if files.is_empty() {
        eprintln!("wireaudit: no .rs files under {roots:?} - pass a different root");
        std::process::exit(2);
    }

    // Every file's text, read once.
    let mut texts: Vec<String> = Vec::with_capacity(files.len());
    for file in &files {
        texts.push(std::fs::read_to_string(file).unwrap_or_default());
    }

    let items = collect_items(&files, &texts);
    let item_count = items.len();
    let mut results: BTreeMap<&'static str, Vec<Item>> = BTreeMap::new();
    for item in items {
        let referenced = referenced_outside(&item, &files, &texts);
        if referenced {
            continue;
        }
        results.entry(item.kind).or_default().push(item);
    }

    let total: usize = results.values().map(|v| v.len()).sum();
    println!("wireaudit");
    println!("  files scanned: {}", files.len());
    println!("  public items:  {item_count}");
    println!("  file-local:    {total}");
    println!();

    if total == 0 {
        println!("Nothing reported. Either everything public is wired, or the");
        println!("filter excluded everything - check what you asked for.");
        return;
    }

    for (kind, list) in &results {
        let shown: Vec<&Item> = match &filter {
            Some(needle) => list.iter().filter(|i| i.file.to_string_lossy().contains(needle)).collect(),
            None => list.iter().collect(),
        };
        if shown.is_empty() {
            continue;
        }
        println!("{} ({} file-local)", kind, shown.len());
        for item in shown {
            println!("  {:<40} {}:{}", item.name, item.file.display(), item.line);
        }
        println!();
    }

    println!("File-local means: no OTHER file names it. It may still be called by");
    println!("its siblings, or by a macro. Each one wants a human look - this is a");
    println!("worklist, not a verdict.");
}

/// Every `.rs` under `dir`, skipping build output and vendored trees.
fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if path.is_dir() {
            // `target` and `assets` hold no sources; skipping them keeps the run
            // fast enough to be worth doing before every change.
            if name == "target" || name == "assets" || name == ".git" {
                continue;
            }
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

/// Finds `pub` items by scanning declarations line by line.
///
/// Deliberately not a parser. The shapes it looks for are the ones this codebase
/// uses, and anything it misses shows up as an item that is simply not reported
/// - a false negative that costs nothing, unlike a false positive that sends
/// someone to read a wired function.
fn collect_items(files: &[PathBuf], texts: &[String]) -> Vec<Item> {
    let mut items = Vec::new();
    for (file, text) in files.iter().zip(texts) {
        for (index, line) in text.lines().enumerate() {
            let trimmed = line.trim_start();

            let kind = if let Some(rest) = trimmed.strip_prefix("pub fn ") {
                Some(("fn", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub unsafe fn ") {
                Some(("fn", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub async fn ") {
                Some(("fn", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub struct ") {
                Some(("struct", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub enum ") {
                Some(("enum", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub trait ") {
                Some(("trait", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub const ") {
                Some(("const", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub static ") {
                Some(("static", rest))
            } else if let Some(rest) = trimmed.strip_prefix("pub mod ") {
                Some(("mod", rest))
            } else {
                None
            };

            let Some((kind, rest)) = kind else { continue };
            // `<` for generics, `(` for a call signature, whitespace for a name.
            let name: String = rest
                .chars()
                .take_while(|c| c.is_alphanumeric() || *c == '_')
                .collect();
            if name.is_empty() || name.starts_with(|c: char| c.is_ascii_digit()) {
                continue;
            }
            items.push(Item {
                name,
                file: file.clone(),
                line: index + 1,
                kind,
            });
        }
    }
    items
}

/// Whether any file other than the defining one mentions `item.name`.
///
/// A whole-word match, so `step` does not count a hit in `stepwise`. The
/// identifier is searched for anywhere in the line rather than only in call
/// position, because a use that is not a call - a field access, a re-export, a
/// trait bound - is still a real edge.
fn referenced_outside(item: &Item, files: &[PathBuf], texts: &[String]) -> bool {
    for (file, text) in files.iter().zip(texts) {
        if file == &item.file {
            continue;
        }
        if file_mentions(text, &item.name) {
            return true;
        }
    }
    false
}

fn file_mentions(text: &str, name: &str) -> bool {
    for line in text.lines() {
        let bytes = line.as_bytes();
        let mut from = 0usize;
        while let Some(offset) = line[from..].find(name) {
            let start = from + offset;
            let end = start + name.len();
            let before_ok = start == 0 || !is_ident_byte(bytes[start - 1]);
            let after_ok = end >= bytes.len() || !is_ident_byte(bytes[end]);
            if before_ok && after_ok {
                return true;
            }
            from = end.max(start + 1);
        }
    }
    false
}

fn is_ident_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}