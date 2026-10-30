//! Reachability: did each subsystem actually run, or was it merely written?
//!
//! Bug №313. Every other test in this suite asks "is the result correct". This one
//! asks a prior question: "was there a result at all".
//!
//! Why it needs its own file. A subsystem can be written, compiled, documented,
//! exported, linted clean and reviewed — and still never execute once. Nothing in
//! the build objects: there are no commands, so there is nothing for the validation
//! layer to complain about, and a green suite is exactly what such a subsystem
//! produces. Five of the seven render passes were in that state, with their
//! pipelines never created and their resources never allocated, while every test
//! passed.
//!
//! So the criterion here is deliberately not "the test is green". It is "this line
//! was called".

use rfs_client::render::core::RenderContext;
use rfs_client::render::core::resource_manager::{
    pipeline_request_is_supported, Format, PipelineDesc, PipelineType, VertexAttribute, VertexLayout,
};
use rfs_client::render::graph::resource::GraphResource;
use rfs_client::render::graph::types::ResourceUsage;
use rfs_client::render::graph::{PassDependency, RenderGraph, RenderPass, RenderPassNode};
use rfs_client::render::scene::Scene;
use rhi::CommandEncoder;
use rhi::core::device::DeviceDesc;
use rhi::types::Features;
use std::collections::HashMap;
use std::sync::Arc;

/// A device, because `RenderContext` owns one.
///
/// Needed only so the graph has somewhere to record into. Nothing here submits, and
/// nothing is rendered — the pass under test does no GPU work at all. That is the
/// point of this file: the question is whether the graph *called* anything, which
/// happens before a single command reaches the driver.
fn headless_context() -> RenderContext {
    let backend = rhi::backend::create_backend(&rhi::RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    assert!(!devices.is_empty(), "no Vulkan device available");
    let device = Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc {
                    features: Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    );
    RenderContext::headless(device)
}

/// A pass that records nothing and does nothing, so a test can observe the graph's
/// own bookkeeping without a GPU, a scene or a real pipeline.
struct CountingPass {
    runs: std::cell::Cell<u32>,
}

impl CountingPass {
    fn new() -> Self {
        Self { runs: std::cell::Cell::new(0) }
    }
}

impl RenderPass for CountingPass {
    fn name(&self) -> &str {
        "counting"
    }
    fn dependencies(&self) -> &[PassDependency] {
        &[]
    }
    fn outputs(&self) -> &[String] {
        &[]
    }
    fn execute(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> rfs_client::render::graph::node::PassOutcome {
        self.runs.set(self.runs.get() + 1);
        // The pass counts its own runs, and it records nothing. It says so, so
        // the graph can tell "was called" from "did the work" — the counting
        // pass is dispatched every frame and contributes nothing, which is
        // exactly the state the instrument has to be able to see.
        rfs_client::render::graph::node::PassOutcome::skipped("the counting pass records nothing")
    }
}

fn node(name: &str) -> RenderPassNode {
    RenderPassNode {
        name: name.into(),
        pass: Box::new(CountingPass::new()),
        dependencies: Vec::new(),
        outputs: Vec::new(),
        enabled: true,
        execution_order: 0,
    }
}

/// `RenderGraph::initialize` must actually initialize its passes. Bug №311.
///
/// This is the test for the gap the counter exposed, so the two belong in one
/// file: the instrument found that the pass bodies never ran, and this proves the
/// fix.
///
/// Before the fix `RenderPassNode::initialize` had **no call sites in the
/// repository** — not here, not in the renderer, nowhere. Every pass's
/// `initialize` body was dead code, so its textures and pipelines never existed and
/// any `execute` guarding on its framebuffer returned early. Adding a pass to a
/// graph could therefore never work, which is the part of "written but not wired"
/// that no amount of review would have caught.
#[test]
fn graph_initialize_reaches_every_pass() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut graph = graph_with_pass(ProbePass {
        label: "init-probe",
        on_initialize: Some(calls.clone()),
        on_resize: None,
    });

    let device = probe_device();
    let context = RenderContext::headless(device.clone());
    let mut scene = Scene::new("probe");
    graph.initialize(&device, &context, &mut scene);

    assert_eq!(
        calls.load(std::sync::atomic::Ordering::SeqCst),
        1,
        "adding a pass to a graph must cause its `initialize` to run. It did not \
         before bug №311: the method had no call sites anywhere in the repository, \
         so the pass's resources were never created and its `execute` would return \
         early forever."
    );
}

/// The same for `resize`, which also had no call sites.
#[test]
fn graph_resize_reaches_every_pass() {
    let sizes: ResizeLog = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut graph = graph_with_pass(ProbePass {
        label: "resize-probe",
        on_initialize: None,
        on_resize: Some(sizes.clone()),
    });

    graph.resize(800, 600);
    graph.resize(1024, 768);

    assert_eq!(
        *sizes.lock().expect("probe lock"),
        vec![(800, 600), (1024, 768)],
        "every resize must reach the pass; `RenderPassNode::resize` had no call \
         sites, so a pass with size-dependent resources would keep the old ones"
    );
}

/// Where a pass sends the sizes it is told about, shared with the test.
type ResizeLog = Arc<std::sync::Mutex<Vec<(u32, u32)>>>;

/// A pass that records the calls it receives. Shared counters rather than fields,
/// because the graph holds it as `Box<dyn RenderPass>` and a test cannot read a
/// field back out through the trait object.
struct ProbePass {
    label: &'static str,
    on_initialize: Option<Arc<std::sync::atomic::AtomicUsize>>,
    on_resize: Option<ResizeLog>,
}

impl RenderPass for ProbePass {
    fn name(&self) -> &str {
        self.label
    }
    fn dependencies(&self) -> &[PassDependency] {
        &[]
    }
    fn outputs(&self) -> &[String] {
        &[]
    }
    fn initialize(
        &mut self,
        _device: &rhi::Device,
        _context: &RenderContext,
        _resources: &mut HashMap<String, GraphResource>,
    ) {
        if let Some(counter) = &self.on_initialize {
            counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
    }
    fn execute(
        &mut self,
        _encoder: &mut CommandEncoder,
        _context: &mut RenderContext,
        _scene: &mut Scene,
        _resources: &HashMap<String, GraphResource>,
    ) -> rfs_client::render::graph::node::PassOutcome {
        // The probe records nothing, and says so.
        rfs_client::render::graph::node::PassOutcome::skipped("the probe records nothing")
    }
    fn resize(&mut self, width: u32, height: u32) {
        if let Some(sizes) = &self.on_resize {
            sizes.lock().expect("probe lock").push((width, height));
        }
    }
}

fn graph_with_pass(pass: ProbePass) -> RenderGraph {
    let mut graph = RenderGraph::new();
    graph.add_pass(RenderPassNode {
        name: pass.label.into(),
        pass: Box::new(pass),
        dependencies: Vec::new(),
        outputs: Vec::new(),
        enabled: true,
        execution_order: 0,
    });
    graph
}

fn probe_device() -> Arc<rhi::Device> {
    let backend = rhi::backend::create_backend(&rhi::RhiConfig::default()).expect("create backend");
    let devices = backend.enumerate_physical_devices().expect("enumerate");
    Arc::new(
        backend
            .create_device(
                &devices[0],
                &DeviceDesc {
                    features: Features::default(),
                    ..Default::default()
                },
            )
            .expect("create device"),
    )
}

/// `Renderer::resize` must reach the passes, not just `RenderGraph::resize`. Bug №311.
///
/// A separate test from the one above, and the reason is worth recording. Removing
/// the `self.render_graph.resize(...)` line from `Renderer::resize` leaves
/// `graph_resize_reaches_every_pass` **green**, because that test drives
/// `RenderGraph::resize` directly. The defect is the same one this file exists for
/// — a method that works perfectly and is never called — and the first version of
/// this test did not catch it. Checked by negative control before the fix was
/// accepted.
#[test]
fn renderer_resize_reaches_the_passes() {
    use rfs_client::render::Renderer;

    let sizes: ResizeLog = Arc::new(std::sync::Mutex::new(Vec::new()));
    // Null window handle: the renderer takes one for the surface, and without one it
    // runs headless. The point of this test is the resize path, which happens
    // before any surface is involved.
    let mut renderer = Renderer::new(std::ptr::null_mut(), rhi::RhiConfig::default())
        .expect("headless renderer needs no window");
    renderer.render_graph_mut().add_pass(RenderPassNode {
        name: "resize-probe".into(),
        pass: Box::new(ProbePass {
            label: "resize-probe",
            on_initialize: None,
            on_resize: Some(sizes.clone()),
        }),
        dependencies: Vec::new(),
        outputs: Vec::new(),
        enabled: true,
        execution_order: 0,
    });

    renderer.resize(1280, 720);

    assert_eq!(
        *sizes.lock().expect("probe lock"),
        vec![(1280, 720)],
        "`Renderer::resize` must forward to the graph. It did not before bug №311, \
         so a pass holding size-dependent resources kept the old ones and kept \
         rendering at the previous size"
    );
}

/// A pass added to the graph must be counted when the graph runs.
#[test]
fn a_pass_added_to_the_graph_is_counted_when_the_graph_runs() {
    let mut graph = RenderGraph::new();
    graph.add_pass(node("counting"));
    graph.mark_dirty();

    // Drive the graph without a device. `record` is the path the pixel tests use,
    // and counting there as well as on `execute` is deliberate — see the note at the
    // counting site in `graph.rs`.
    let mut context = headless_context();
    let mut scene = Scene::new("probe");
    let commands = graph.record(&mut context, &mut scene);

    let counts = graph.pass_execution_counts();
    assert_eq!(
        counts,
        vec![("counting".to_string(), 1)],
        "the pass was added and the graph recorded, so it must be counted once"
    );
    assert_eq!(
        graph.never_executed(),
        Vec::<String>::new(),
        "a pass that just ran cannot also be in the never-executed list"
    );
    assert!(
        !commands.is_empty() || graph.pass_execution_counts()[0].1 == 1,
        "sanity: recording happened"
    );
}

/// Repeated runs accumulate rather than saturating.
///
/// A counter that reported "ran" rather than "how often" would pass the previous
/// test and tell us nothing about a per-frame path.
#[test]
fn repeated_runs_accumulate() {
    let mut graph = RenderGraph::new();
    graph.add_pass(node("counting"));
    graph.mark_dirty();
    let mut context = headless_context();
    let mut scene = Scene::new("probe");

    for _ in 0..3 {
        graph.record(&mut context, &mut scene);
    }

    assert_eq!(
        graph.pass_execution_counts(),
        vec![("counting".to_string(), 3)],
        "three runs must count three"
    );
}

/// A disabled pass is reported as never executed, and says so.
///
/// This is the distinction that makes the list actionable. "Disabled" and "never
/// reached" both read `0`, and pretending otherwise would make the list a place
/// where two different bugs hide behind one answer.
#[test]
fn a_disabled_pass_is_reported_as_never_executed() {
    let mut graph = RenderGraph::new();
    let mut disabled = node("off");
    disabled.enabled = false;
    graph.add_pass(disabled);
    graph.add_pass(node("on"));
    graph.mark_dirty();

    let mut context = headless_context();
    let mut scene = Scene::new("probe");
    graph.record(&mut context, &mut scene);

    assert_eq!(
        graph.never_executed(),
        vec!["off".to_string()],
        "the disabled pass is the one that did not run, and it must be named"
    );
    assert_eq!(
        graph.pass_execution_counts(),
        vec![
            ("off".to_string(), 0),
            ("on".to_string(), 1),
        ],
        "counts are in execution order so a wrong order is visible"
    );
}

/// Bug №653: "was called" and "did the work" are different questions, and the
/// instrument has to answer both.
///
/// The counter used to be incremented *before* `execute` was called, so a pass
/// that returned early — no framebuffer, no pipeline, no state — was counted
/// as having run, while its doc comment claimed the counts were "of real
/// executions". Five of the seven passes were in exactly that state: their
/// pipelines were never created, so their `execute` returned before recording
/// anything, and the counter said they ran every frame.
///
/// So the graph counts two numbers now, and the second one is the finding.
#[test]
fn a_pass_that_was_called_and_did_nothing_is_reported_as_such() {
    let mut graph = RenderGraph::new();
    // The counting pass records nothing, and says so.
    graph.add_pass(node("counting"));
    graph.mark_dirty();
    let mut context = headless_context();
    let mut scene = Scene::new("probe");

    graph.record(&mut context, &mut scene);

    assert_eq!(
        graph.pass_execution_counts(),
        vec![("counting".to_string(), 1)],
        "the pass was dispatched, so the dispatch count is one"
    );
    assert_eq!(
        graph.recorded_counts(),
        vec![("counting".to_string(), 0)],
        "the pass recorded nothing, so the recorded count is zero — and that is \
         the number the old instrument could not produce"
    );
    assert_eq!(
        graph.passes_without_work(),
        vec![(
            "counting".to_string(),
            "the counting pass records nothing".to_string()
        )],
        "a pass that was called and did nothing has to be named, with the reason"
    );
    assert!(
        graph.never_executed().is_empty(),
        "the pass *was* reached, so it is not in the never-reached list; the two \
         lists are different failures and must not be conflated"
    );
    assert!(
        graph.execution_summary().contains("countingx1!0"),
        "the summary has to show that the pass was called once and recorded \
         nothing, got: {}",
        graph.execution_summary()
    );
}

/// Bug №653: a pass in a dependency cycle must not vanish.
///
/// Kahn's algorithm places every node it can and silently drops the rest. A
/// pass in a cycle — or one that depends on itself — never reaches in-degree
/// zero, so it never entered the execution order, and since the counters were
/// read off that order, it was invisible to the instrument built to find
/// exactly that.
#[test]
fn a_pass_in_a_dependency_cycle_is_still_reported() {
    let mut graph = RenderGraph::new();
    // Two passes that depend on each other: neither can ever be ordered.
    let mut a = node("a");
    a.dependencies = vec![PassDependency::new("b", "", ResourceUsage::NONE)];
    let mut b = node("b");
    b.dependencies = vec![PassDependency::new("a", "", ResourceUsage::NONE)];
    graph.add_pass(a);
    graph.add_pass(b);
    graph.mark_dirty();

    let mut context = headless_context();
    let mut scene = Scene::new("probe");
    graph.record(&mut context, &mut scene);

    let counts = graph.pass_execution_counts();
    let names: Vec<&str> = counts.iter().map(|(name, _)| name.as_str()).collect();
    assert_eq!(
        names,
        vec!["a", "b"],
        "a pass in a cycle must still be reported; the sort used to drop it \
         silently, and a pass the instrument cannot see is a pass the \
         instrument cannot find"
    );
}

/// The real finding, asserted rather than written up in a document.
///
/// Every pass the render layer declares must be constructed somewhere that is not
/// its own file. This is the check that would have named five empty subsystems in
/// one run instead of one per investigation.
///
/// It reads sources rather than executing them, which is a different guarantee from
/// the tests above: those prove the counting works, this one proves the declaration
/// list is not larger than reality. Neither substitutes for the other.
#[test]
fn every_declared_pass_is_instantiated_somewhere() {
    let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("locate the repository root from the test manifest");

    // Passes that are known not to be wired, with the reason. An entry here is a
    // debt with a name on it, which is the difference between a list and an excuse.
    // Both directions of this check were verified by negative control before the
    // controls were removed: dropping a genuinely unwired pass (`WaterPass`) made it
    // name the pass, and listing a wired one (`GeometryPass`) made the staleness
    // assertion fire. A check that only ever passed proves nothing.
    let known_unwired: &[(&str, &str)] = &[
        // `GBufferPass` and `LightingPass` were removed together with the
        // deferred chain (№741): the same picture has been proven on the
        // forward path (`forward_pixels.rs`, `forward_clusters.rs`), so the
        // second path was deleted rather than maintained.
        // `ShadowPass` used to be here: "depth pipeline never written". It is
        // not any more — the pass has a real depth-only pipeline, a real
        // shadow map with the scene's depth in it, per-item uniform blocks,
        // and a pixel test that reads the map back and requires the geometry
        // to be in it (`Tool/client_tests/tests/shadow_pixels.rs`). The
        // allowlist entry failed on its own staleness check, which is what it
        // is for.
        //
        // What the shadow pass does *not* have yet is a consumer: nothing
        // samples the map, because the forward lighting shader that would
        // read it is the next step of the Forward+ plan. That is a missing
        // feature and not a stub, and it is tracked in the TODO.
        // `TransparentPass` used to be here: "pipelines map is created empty
        // and never filled". It is not any more — the pass has four real blend
        // pipelines (one per blend mode), per-object uniform blocks, a real
        // drawing loop, the depth it tests against, and a pixel test that
        // reads the result back and requires the blend to have happened
        // (`Tool/client_tests/tests/transparent_pixels.rs`). The allowlist
        // entry failed on its own staleness check, which is what it is for.
        //
        // What the transparent pass does *not* have yet is a place in the
        // product's frame: it draws over the image the opaque pass produced,
        // and the forward opaque pass does not exist yet (the Forward+ plan in
        // the TODO). That is a missing feature, not a stub.
        // `WaterPass` used to be here: "water pipeline never written". It is
        // not any more — the pass has a real pipeline, a real water surface
        // with waves in the vertex stage, generated wave normals, the
        // reflection and the refraction it mixes by the Fresnel term, and a
        // pixel test that reads the water's own image back
        // (`Tool/client_tests/tests/water_pixels.rs`).
        //
        // What the water pass does *not* have yet is whoever renders the
        // scene into the refraction: a pass cannot render the scene, it can
        // only draw what it is given. The reflection side got its renderer:
        // `MirrorPass` builds and drives the mirrored camera itself (wired
        // №721; `Tool/client_tests/tests/mirror_pixels.rs` proves the picture
        // it renders is the *mirrored* camera's, not the main one), so what
        // is still missing is only the under-water view the refraction wants.
        //
        // That is the last of the three. The allowlist is empty.
    ];

    let pass_dir = manifest.join("src").join("render").join("passes");
    let mut declared: Vec<String> = std::fs::read_dir(&pass_dir)
        .expect("passes directory")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().to_string())
        .filter(|f| f.ends_with(".rs") && f != "mod.rs" && f != "base.rs")
        .collect();
    declared.sort();

    let mut unwired = Vec::new();
    for file in &declared {
        let own_path = pass_dir.join(file);
        let Ok(text) = std::fs::read_to_string(&own_path) else {
            continue;
        };
        // The type name is read out of the file, not derived from its name.
        // `ui.rs` declares `UIPass`; camelising the filename gives `Ui`, which
        // matches neither the type nor the `known_unwired` list, and a check
        // that misidentifies its own subject is worse than none.
        let Some(type_name) = declared_pass_type(&text) else {
            continue;
        };

        let mut constructed_outside = false;
        for entry in walk(manifest) {
            if entry == own_path {
                continue;
            }
            let Ok(other) = std::fs::read_to_string(&entry) else {
                continue;
            };
            if other.contains(&format!("{type_name}::"))
                || other.contains(&format!("{type_name} {{"))
                || other.contains(&format!("Box<{type_name}"))
            {
                constructed_outside = true;
                break;
            }
        }
        if !constructed_outside && !known_unwired.iter().any(|(k, _)| *k == type_name) {
            unwired.push(type_name.to_string());
        }
    }

    assert!(
        unwired.is_empty(),
        "these passes exist as files but are constructed nowhere outside themselves, \
         so their code has never run and no test can be exercising it: {unwired:?}. \
         Either wire them or add them to the `known_unwired` list with the reason — \
         an unnamed gap is how five subsystems stayed invisible."
    );

    // Every entry must still be honest: if a listed pass starts being constructed,
    // the reason is stale and should be removed rather than left to rot.
    for (name, reason) in known_unwired {
        let mut now_wired = false;
        for entry in walk(manifest) {
            if entry.file_name().and_then(|n| n.to_str()) == Some("mod.rs")
                && entry.to_string_lossy().contains("render")
            {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&entry) else {
                continue;
            };
            if text.contains(&format!("{name}::")) {
                now_wired = true;
                break;
            }
        }
        assert!(
            !now_wired,
            "{name} is listed as unwired ({reason}) but is now constructed somewhere. \
             Remove it from the list so the debt stays accurate."
        );
    }
}

fn declared_pass_type(text: &str) -> Option<&str> {
    // The pass struct is the one named `*Pass`; `RenderPass` itself is a trait in
    // another module and `*PassState` is not a pass.
    for line in text.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("pub struct ") else {
            continue;
        };
        let name: String = rest.chars().take_while(|c| c.is_alphanumeric()).collect();
        if name.ends_with("Pass") && !name.ends_with("PassState") {
            return Some(Box::leak(name.into_boxed_str()));
        }
    }
    None
}

fn walk(root: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            // Every `target*` directory, not just `target`: the agents build into
            // `target-sbf1` and `target-sbf2`, and walking those would read hundreds
            // of megabytes of build output for nothing.
            if name.starts_with('.') || name.starts_with("target") || name == "UPl" {
                continue;
            }
            if path.is_dir() {
                stack.push(path);
            } else if name.ends_with(".rs") {
                out.push(path);
            }
        }
    }
    out
}

/// A pipeline request that cannot be honoured as written.
///
/// Bug №314: `ResourceManager::create_pipeline` answered a compute request by
/// building a graphics pipeline, and answered a request that declared a
/// geometry stage by dropping the stage without a word. Both produce a green
/// build and a wrong frame, which is the failure mode this file exists to catch.
///
/// Tested through the pure predicate rather than through `create_pipeline`,
/// because that function returns `None` for many unrelated reasons — a missing
/// file, a missing native handle — and an assertion on `None` cannot say which
/// guard fired. That is not theoretical: written against `create_pipeline`, these
/// assertions stayed green after the type guard was disabled outright, because
/// another check was already returning `None`.
fn pipeline_desc(kind: PipelineType) -> PipelineDesc {
    PipelineDesc {
        name: "reachability".to_string(),
        vertex_shader: "vs".to_string(),
        vertex_shader_path: "assets/shaders/fullscreen.spv".to_string(),
        fragment_shader: None,
        fragment_shader_path: None,
        geometry_shader: None,
        compute_shader: None,
        vertex_layout: VertexLayout {
            attributes: vec![VertexAttribute {
                name: "position".to_string(),
                format: Format::R32G32B32A32Float,
                offset: 0,
            }],
        },
        render_pass: "geometry".to_string(),
        render_pass_desc: None,
        pipeline_type: kind,
    }
}

#[test]
fn a_compute_request_is_refused_rather_than_built_as_graphics() {
    assert!(
        !pipeline_request_is_supported("compute", &pipeline_desc(PipelineType::Compute)),
        "a compute request must not be answered with a graphics pipeline"
    );
}

#[test]
fn a_declared_geometry_stage_is_refused_rather_than_silently_dropped() {
    let mut d = pipeline_desc(PipelineType::Graphics);
    d.geometry_shader = Some("gs".to_string());

    assert!(
        !pipeline_request_is_supported("with_geometry", &d),
        "a geometry stage is never added to the stage list, so building this \
         pipeline would produce one that ignores its own declared stage"
    );
}

#[test]
fn an_ordinary_graphics_request_is_accepted() {
    // The negative control for the two above. Without it, a predicate that
    // refused everything would satisfy them both and prove nothing.
    assert!(pipeline_request_is_supported(
        "ordinary",
        &pipeline_desc(PipelineType::Graphics)
    ));
}