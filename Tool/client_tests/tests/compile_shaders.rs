//! Compiles the WGSL shader sources to SPIR-V ahead of time.
//!
//! The user decided against moving `naga` into the `rhi` crate, so SPIR-V is
//! produced here and committed as a `.spv` file. That keeps the runtime free
//! of a shader compiler while still letting the sources stay readable WGSL.
//!
//! Run it explicitly after editing any `.wgsl`:
//!
//! ```text
//! cargo test -p client_tests --test compile_shaders -- --ignored --nocapture
//! ```
//!
//! It is `#[ignore]`d on purpose: a normal `cargo test` run must not rewrite
//! committed assets as a side effect, or a test run could silently change what
//! the render tests are actually exercising.

use std::path::PathBuf;

/// WGSL source -> SPIR-V bytes, the same path the render tests used before the
/// sources were pre-compiled. Kept here so the generator and the check below
/// cannot drift apart.
pub fn wgsl_to_spirv(source: &str) -> Vec<u8> {
    let module = naga::front::wgsl::parse_str(source).expect("WGSL parse");
    let mut validator = naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::all(),
    );
    let info = validator.validate(&module).expect("WGSL validation");
    let words = naga::back::spv::write_vec(
        &module,
        &info,
        &naga::back::spv::Options::default(),
        None,
    )
    .expect("SPIR-V emit");

    let mut bytes = Vec::with_capacity(words.len() * 4);
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn asset_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets").join("shaders")
}

/// Every `.wgsl` next to a committed `.spv`, kept in step with each other.
fn shader_pairs() -> Vec<(PathBuf, PathBuf)> {
    let dir = asset_dir();
    let mut pairs: Vec<_> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "wgsl"))
        .map(|p| {
            let spv = p.with_extension("spv");
            (p, spv)
        })
        .collect();
    pairs.sort();
    assert!(!pairs.is_empty(), "no .wgsl sources in {}", dir.display());
    pairs
}

#[test]
#[ignore = "regenerates committed .spv assets; run explicitly after editing WGSL"]
fn compile_shaders() {
    for (src, spv) in shader_pairs() {
        let source = std::fs::read_to_string(&src).expect("read WGSL");
        let bytes = wgsl_to_spirv(&source);
        std::fs::write(&spv, &bytes).expect("write SPIR-V");
        println!("{} -> {} ({} bytes)", src.display(), spv.display(), bytes.len());
    }
}

/// Runs in a normal `cargo test` pass. Fails if a committed `.spv` is missing,
/// which is the case a fresh checkout or a forgotten regeneration hits.
#[test]
fn committed_spirv_is_present_and_well_formed() {
    for (src, spv) in shader_pairs() {
        let bytes = std::fs::read(&spv).unwrap_or_else(|e| {
            panic!(
                "{} has no compiled .spv ({e}).\nRun: cargo test -p client_tests \
                 --test compile_shaders -- --ignored --nocapture",
                src.display()
            )
        });

        // SPIR-V is five words of header — magic, version, generator, bound,
        // schema — followed by instructions. There is *no* length field: the
        // module simply ends with the file. So the checks that are meaningful
        // are the magic, a known version, and whole-word size.
        assert!(bytes.len() % 4 == 0, "{}: not word aligned ({} bytes)", spv.display(), bytes.len());
        assert!(
            bytes.len() >= 20,
            "{}: shorter than the 5-word header ({} bytes)",
            spv.display(),
            bytes.len()
        );

        let word = |i: usize| u32::from_le_bytes([bytes[i * 4], bytes[i * 4 + 1], bytes[i * 4 + 2], bytes[i * 4 + 3]]);
        assert_eq!(word(0), 0x0723_0203, "{}: bad SPIR-V magic {:#x}", spv.display(), word(0));

        // Major version 1 is the only one that exists; 0.x was the pre-release
        // numbering and is not valid input.
        let version = word(1);
        assert_eq!(
            version >> 16,
            1,
            "{}: unexpected SPIR-V version {:#x}",
            spv.display(),
            version
        );
        assert!(
            version & 0xFFFF <= 0xFF,
            "{}: implausible SPIR-V version {:#x}",
            spv.display(),
            version
        );
    }
}
