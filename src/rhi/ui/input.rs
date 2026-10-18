//! Input Handling
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Turns raw input events into element state: hover, press, click, focus,
//! scrolling and text entry.
//!
//! # Ordering
//!
//! Hit testing walks the **reverse** of the draw order, so the element that was
//! drawn last — the one visually on top — is the one that receives the click.
//! Both use [`draw_order`], which is the single definition of "what is on top".
//! Two independent orderings would put hover on one element and the click on
//! another, which reads as the UI being broken in a way that is hard to diagnose.
//!
//! # Keys
//!
//! [`UiKey`] is this crate's own enumeration rather than a window-system key
//! code. `rhi` is a hardware abstraction and must not depend on winit or Win32;
//! the application layer translates its event into these.
//!
//! # Scope
//!
//! Implemented: hover, press, release, click, focus traversal, Tab order,
//! character entry, Backspace, scroll accumulation.
//!
//! Not implemented: double-click timing, drag and drop, right-click menus,
//! scroll clipping in hit testing (an element scrolled out of a container is
//! still hit inside the container's rect).

use std::collections::HashMap;

use super::elements::{UiElement, UiElementData, UiElementId};

/// Keyboard modifier states.
///
/// Lived in `context.rs` next to a `UiContext` that nothing ever constructed.
/// Kept here instead, because this is the only place that reads it: focus
/// traversal goes backwards on shift, and that is the whole of it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct UiModifiers {
    pub shift: bool,
    pub control: bool,
    pub alt: bool,
    pub super_key: bool,
}

/// A key the UI reacts to.
///
/// Only keys with defined UI meaning appear here. Printable characters arrive
/// through [`UiInputEvent::CharacterReceived`] instead, because they come from
/// text layout rather than from physical key state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum UiKey {
    Backspace,
    Delete,
    Enter,
    Tab,
    Escape,
    Left,
    Right,
    Up,
    Down,
    Home,
    End,
    PageUp,
    PageDown,
}

/// An input event, already in screen pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum UiInputEvent {
    MouseMoved {
        x: f32,
        y: f32,
    },
    MousePressed {
        x: f32,
        y: f32,
        button: usize,
    },
    MouseReleased {
        x: f32,
        y: f32,
        button: usize,
    },
    Scroll {
        delta: f32,
    },
    KeyPressed {
        key: UiKey,
    },
    /// Modifier keys changed.
    ///
    /// Its own event rather than a field on `KeyPressed`, because a window
    /// reports "shift is down" independently of which key was pressed, and
    /// `KeyPressed` with no matching release is the wrong way to model it.
    ///
    /// This was missing: `state.modifiers.shift` was read by focus traversal,
    /// and the only thing that ever set it was a test, so shift-tab went
    /// forwards in the game exactly as plain tab did.
    ModifiersChanged {
        modifiers: UiModifiers,
    },
    CharacterReceived(char),
}

/// Input state carried between frames.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiInputState {
/// Cursor in screen pixels.
///
/// A tuple rather than an array to match [`UiElement::position`], so the two can
/// be compared and passed around without converting at every boundary.
pub mouse: (f32, f32),
    /// Whether the cursor is inside the UI area at all.
    ///
    /// Separate from `mouse` because a cursor parked at (0,0) with the window
    /// unfocused is not hovering anything; without this, alt-tabbing away would
    /// leave the first element stuck in its hover state.
    pub cursor_present: bool,
    pub buttons: [bool; 3],
    pub modifiers: UiModifiers,
    /// Element under the cursor.
    pub hovered: Option<UiElementId>,
    /// Element holding a press, from press until release.
    pub active: Option<UiElementId>,
    /// Element holding keyboard focus.
    pub focused: Option<UiElementId>,
}

impl UiInputState {
    /// Forget the cursor, e.g. when the window loses focus.
    ///
    /// Buttons are released too: a press held while the window is deactivated
    /// would otherwise stay `active` forever, because the matching release is
    /// delivered to whichever window has focus.
    pub fn release_all(&mut self) {
        self.buttons = [false; 3];
        self.cursor_present = false;
        self.active = None;
    }
}

/// A completed click.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiClick {
    pub element: UiElementId,
    pub button: usize,
    pub position: (f32, f32),
}

/// What one update produced.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct UiInteraction {
    pub hovered: Option<UiElementId>,
    pub active: Option<UiElementId>,
    pub focused: Option<UiElementId>,
    /// Clicks completed during this update.
    pub clicks: Vec<UiClick>,
    /// Scroll not consumed by any element, in lines.
    pub scroll: f32,
    /// Characters accepted into the focused element during this update.
    pub text: String,
    /// Focus moved during this update, if it did.
    pub focus_moved: bool,
}

/// Elements in the order they are drawn: ascending `z_index`, then ascending id.
///
/// The id tiebreak exists because a `HashMap` has no order, and without one the
/// same frame would composite differently on two runs — panels flickering as
/// they win and lose by luck.
pub fn draw_order(elements: &HashMap<UiElementId, UiElement>) -> Vec<UiElementId> {
    let mut ids: Vec<UiElementId> = elements.keys().copied().collect();
    ids.sort_by(|a, b| {
        let za = elements.get(a).map_or(0, |e| e.style.z_index);
        let zb = elements.get(b).map_or(0, |e| e.style.z_index);
        za.cmp(&zb).then(a.cmp(b))
    });
    ids
}

/// The topmost interactive element at `point`, if any.
///
/// "Topmost" is the last element in draw order that contains the point, which
/// is the one drawn last and therefore visible above the others.
pub fn hit_test(
    elements: &HashMap<UiElementId, UiElement>,
    rects: &HashMap<UiElementId, super::layout::UiRect>,
    point: (f32, f32),
) -> Option<UiElementId> {
    let (x, y) = point;
    if !(x.is_finite() && y.is_finite()) {
        return None;
    }

    draw_order(elements)
        .into_iter()
        .rev()
        .find(|id| {
            elements.get(id).is_some_and(|e| {
                e.is_interactive()
                    && rects
                        .get(id)
                        .is_some_and(|&rect| super::layout::rect_contains(e, rect, x, y))
            })
        })
}

/// The element focus would move to, one step in `direction`.
///
/// Cycles, wrapping at both ends, over the elements that can take focus.
pub fn next_focusable(
    elements: &HashMap<UiElementId, UiElement>,
    from: Option<UiElementId>,
    forward: bool,
) -> Option<UiElementId> {
    let order = draw_order(elements);
    let focusable: Vec<UiElementId> = order
        .into_iter()
        .filter(|id| {
            elements
                .get(id)
                .is_some_and(|e| e.is_interactive() && !matches!(e.element_type, super::elements::UiElementType::Panel))
        })
        .collect();

    if focusable.is_empty() {
        return None;
    }

    match from {
        None => focusable.first().copied(),
        Some(current) => {
            let index = focusable.iter().position(|id| *id == current);
            match index {
                Some(i) => {
                    let next = if forward {
                        (i + 1) % focusable.len()
                    } else {
                        // Wrapping backwards from the first element lands on the
                        // last, which `(i + len - 1) % len` also gives without
                        // a branch on zero.
                        (i + focusable.len() - 1) % focusable.len()
                    };
                    focusable.get(next).copied()
                }
                // Focus pointed at something that has since become
                // unfocusable: start from the top rather than nowhere.
                None => focusable.first().copied(),
            }
        }
    }
}

/// Apply a batch of events, updating element state.
///
/// Call once per frame, before rendering, so the flags a pass reads describe
/// this frame's input.
pub fn update(
    elements: &mut HashMap<UiElementId, UiElement>,
    state: &mut UiInputState,
    events: &[UiInputEvent],
    root: super::layout::UiRect,
) -> UiInteraction {
    // Resolved once per frame and shared with the renderer, so a click is
    // always resolved against the rectangle that was actually painted.
    let rects = super::layout::resolve_rects(elements, root);
    let mut out = UiInteraction::default();
    let mut clicks: Vec<UiClick> = Vec::new();
    let mut scroll = 0.0f32;
    let mut text = String::new();
    let mut focus_moved = false;

    for event in events {
        match *event {
            UiInputEvent::MouseMoved { x, y } => {
                state.mouse = (x, y);
                state.cursor_present = x.is_finite() && y.is_finite();
            }
            UiInputEvent::MousePressed { x, y, button } => {
                state.mouse = (x, y);
                state.cursor_present = x.is_finite() && y.is_finite();
                if button < state.buttons.len() {
                    state.buttons[button] = true;
                }
                // Pressing anywhere means the cursor is present even if no
                // move event preceded it, which is common with synthetic input
                // and with focus gained by click.
                let target = if state.cursor_present {
                    hit_test(elements, &rects, state.mouse)
                } else {
                    None
                };
                state.active = target;
                if target.is_some() && state.focused != target {
                    state.focused = target;
                    focus_moved = true;
                }
            }
            UiInputEvent::MouseReleased { x, y, button } => {
                state.mouse = (x, y);
                if button < state.buttons.len() {
                    state.buttons[button] = false;
                }
                // A click is a release over the element that took the press.
                // Requiring the same element is what stops a drag that ends
                // elsewhere from firing the button.
                if state.active.is_some() && button == 0 {
                    let over = if state.cursor_present {
                        hit_test(elements, &rects, state.mouse)
                    } else {
                        None
                    };
                    // `over == state.active` alone is not enough: both are
                    // `None` when the cursor left the UI, which would report a
                    // click on nothing.
                    if let (Some(target), Some(active)) = (over, state.active) {
                        if target == active {
                            clicks.push(UiClick {
                                element: target,
                                button,
                                position: state.mouse,
                            });
                        }
                    }
                }
                state.active = None;
            }
            UiInputEvent::Scroll { delta } => {
                if delta.is_finite() {
                    scroll += delta;
                }
            }
            UiInputEvent::ModifiersChanged { modifiers } => {
                state.modifiers = modifiers;
            }
            UiInputEvent::KeyPressed { key } => match key {
                UiKey::Tab => {
                    let next = next_focusable(elements, state.focused, !state.modifiers.shift);
                    state.focused = next;
                    focus_moved = true;
                }
                UiKey::Escape => {
                    if state.focused.is_some() {
                        state.focused = None;
                        focus_moved = true;
                    }
                }
                UiKey::Backspace => {
                    if let Some(id) = state.focused {
                        if let Some(element) = elements.get_mut(&id) {
                            if let UiElementData::InputField { value, .. } = &mut element.data {
                                if let Some(c) = value.pop() {
                                    text.push(c);
                                }
                            }
                        }
                    }
                }
                UiKey::Enter | UiKey::Delete | UiKey::Left | UiKey::Right | UiKey::Up
                | UiKey::Down | UiKey::Home | UiKey::End | UiKey::PageUp | UiKey::PageDown => {}
            },
            UiInputEvent::CharacterReceived(ch) => {
                // Control characters are layout's business, not text entry's.
                if ch.is_control() {
                    continue;
                }
                if let Some(id) = state.focused {
                    if let Some(element) = elements.get_mut(&id) {
                        if let UiElementData::InputField {
                            value,
                            max_length,
                            ..
                        } = &mut element.data
                        {
                            let at_limit = max_length.is_some_and(|m| value.chars().count() >= m);
                            if !at_limit {
                                value.push(ch);
                                text.push(ch);
                            }
                        }
                    }
                }
            }
        }
    }

    // Hover follows the cursor, or is cleared when the cursor has left.
    state.hovered = if state.cursor_present {
        hit_test(elements, &rects, state.mouse)
    } else {
        None
    };

    // Write the resolved state back onto the elements. Done here rather than
    // per-event so a press and release inside one frame cannot leave an
    // element stuck in a state the final cursor position contradicts.
    let hovered = state.hovered;
    let active = state.active;
    let focused = state.focused;
    for (id, element) in elements.iter_mut() {
        element.hovered = Some(*id) == hovered;
        element.pressed = Some(*id) == active;
        element.focused = Some(*id) == focused;
    }

    // Scrolling goes to the focused container, otherwise it stays unconsumed
    // for whatever is underneath to deal with.
    if scroll != 0.0 {
        if let Some(id) = focused {
            if let Some(element) = elements.get_mut(&id) {
                if let UiElementData::ScrollContainer { scroll_y, .. } = &mut element.data {
                    *scroll_y -= scroll;
                    scroll = 0.0;
                }
            }
        }
    }

    out.hovered = hovered;
    out.active = active;
    out.focused = focused;
    out.clicks = clicks;
    out.scroll = scroll;
    out.text = text;
    out.focus_moved = focus_moved;
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ui::elements::UiElementType;
    use crate::ui::layout::UiConstraints;

    /// The containing box the tests lay elements out in. A root at the origin
    /// with default anchor and pivot resolves to `position` unchanged, so these
    /// tests still mean what they meant before rectangles were resolved.
    fn big_root() -> super::super::layout::UiRect {
        (0.0, 0.0, 10_000.0, 10_000.0)
    }

    fn rects_of(m: &HashMap<UiElementId, UiElement>) -> HashMap<UiElementId, super::super::layout::UiRect> {
        super::super::layout::resolve_rects(m, big_root())
    }

    /// `update` against a large root box, so these tests keep the meaning they
    /// had before rectangles were resolved from the tree.
    fn update_in(
        m: &mut HashMap<UiElementId, UiElement>,
        s: &mut UiInputState,
        events: &[UiInputEvent],
    ) -> UiInteraction {
        update(m, s, events, big_root())
    }

    /// A plain clickable button at `rect`.
    fn button(id: u64, rect: [f32; 4]) -> UiElement {
        let mut e = UiElement::new(UiElementType::Button, id);
        e.interactive = true;
        e.position = (rect[0], rect[1]);
        e.size = (rect[2], rect[3]);
        e.data = UiElementData::Button {
            text: format!("b{id}"),
            normal_color: [0.3; 4],
            hover_color: [0.4; 4],
            pressed_color: [0.2; 4],
            disabled_color: [0.1; 4],
        };
        e
    }

    fn field(id: u64, rect: [f32; 4], value: &str) -> UiElement {
        let mut e = UiElement::new(UiElementType::InputField, id);
        e.interactive = true;
        e.position = (rect[0], rect[1]);
        e.size = (rect[2], rect[3]);
        e.data = UiElementData::InputField {
            placeholder: String::new(),
            value: value.to_string(),
            max_length: None,
            multiline: false,
            password: false,
        };
        e
    }

    fn with_z(mut e: UiElement, z: i32) -> UiElement {
        e.style.z_index = z;
        e
    }

    /// Input state with `id` already focused: the usual starting point for key
    /// and text-entry tests.
    fn focused(id: UiElementId) -> UiInputState {
        UiInputState {
            focused: Some(id),
            ..Default::default()
        }
    }

    #[test]
    fn a_point_inside_an_element_hits_it() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        assert_eq!(hit_test(&m, &rects_of(&m), (50.0, 25.0)), Some(1));
    }

    #[test]
    fn a_point_outside_every_element_hits_nothing() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        assert_eq!(hit_test(&m, &rects_of(&m), (500.0, 500.0)), None);
    }

    #[test]
    fn a_non_interactive_element_is_not_hit() {
        // A panel covering a button must not steal its clicks, otherwise every
        // screen with a background becomes unclickable.
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 100.0]));
        let mut panel = UiElement::panel(2);
        panel.position = (0.0, 0.0);
        panel.size = (100.0, 100.0);
        m.insert(2, panel);
        assert_eq!(hit_test(&m, &rects_of(&m), (50.0, 50.0)), Some(1));
    }

    #[test]
    fn a_hidden_or_disabled_element_is_not_hit() {
        let mut m = HashMap::new();
        let mut hidden = button(1, [0.0, 0.0, 10.0, 10.0]);
        hidden.style.visible = false;
        m.insert(1, hidden);
        assert_eq!(hit_test(&m, &rects_of(&m), (5.0, 5.0)), None);

        let mut m = HashMap::new();
        let mut disabled = button(1, [0.0, 0.0, 10.0, 10.0]);
        disabled.enabled = false;
        m.insert(1, disabled);
        assert_eq!(hit_test(&m, &rects_of(&m), (5.0, 5.0)), None);
    }

    #[test]
    fn the_topmost_overlapping_element_wins() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 100.0]));
        // Drawn later, so on top.
        m.insert(2, with_z(button(2, [0.0, 0.0, 100.0, 100.0]), 10));
        assert_eq!(hit_test(&m, &rects_of(&m), (50.0, 50.0)), Some(2));
    }

    #[test]
    fn z_index_beats_insertion_order() {
        // A lower id drawn last is underneath, because z_index decides. Getting
        // this backwards makes the click land on a hidden element.
        let mut m = HashMap::new();
        m.insert(1, with_z(button(1, [0.0, 0.0, 100.0, 100.0]), 5));
        m.insert(2, with_z(button(2, [0.0, 0.0, 100.0, 100.0]), 1));
        assert_eq!(hit_test(&m, &rects_of(&m), (50.0, 50.0)), Some(1));
    }

    #[test]
    fn draw_order_is_stable_and_ascending_by_z_then_id() {
        let mut m = HashMap::new();
        m.insert(3, with_z(button(3, [0.0; 4]), 0));
        m.insert(1, with_z(button(1, [0.0; 4]), 1));
        m.insert(2, with_z(button(2, [0.0; 4]), 0));
        assert_eq!(draw_order(&m), vec![2, 3, 1]);
    }

    #[test]
    fn a_non_finite_point_hits_nothing() {
        // A NaN would make every comparison false, so a naive `contains_point`
        // sweep would pick the first element instead of none.
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 100.0]));
        assert_eq!(hit_test(&m, &rects_of(&m), (f32::NAN, 5.0)), None);
        assert_eq!(hit_test(&m, &rects_of(&m), (5.0, f32::INFINITY)), None);
    }

/// Anchor and pivot were stored on every element and read by nobody.
///
/// Both drawing and hit testing used `position` directly, so the two agreed
/// only because both were wrong. This checks the field is honoured and that a
/// click lands where the element was actually placed.
#[test]
fn anchor_and_pivot_are_resolved_from_the_tree() {
    let mut m = HashMap::new();
    let mut centred = button(1, [0.0, 0.0, 100.0, 40.0]);
    // Centre of the root box, pivot at the element's own centre.
    centred.anchor = (0.5, 0.5);
    centred.pivot = (0.5, 0.5);
    m.insert(1, centred);

    let rects = super::super::layout::resolve_rects(&m, (0.0, 0.0, 200.0, 200.0));
    assert_eq!(
        rects.get(&1),
        Some(&(50.0, 80.0, 100.0, 40.0)),
        "anchor/pivot were ignored - the rect is still the raw position"
    );

    // A point inside where the element was really placed.
    assert_eq!(
        hit_test(&m, &rects, (100.0, 100.0)),
        Some(1),
        "the click did not follow the resolved rect"
    );

    // The negative control: this is inside the *unresolved* rectangle, which is
    // where the click used to be accepted. If this starts passing, the test
    // above is passing for the wrong reason.
    assert_eq!(
        hit_test(&m, &rects, (10.0, 10.0)),
        None,
        "a click still lands at the raw position"
    );
}

/// The default anchor and pivot must resolve to exactly what the old code used,
/// so enabling resolution cannot silently move anything already laid out.
#[test]
fn default_anchor_and_pivot_leave_the_position_alone() {
    let mut m = HashMap::new();
    m.insert(1, button(1, [37.0, 91.0, 100.0, 40.0]));

    let rects = super::super::layout::resolve_rects(&m, (0.0, 0.0, 1920.0, 1080.0));
    assert_eq!(rects.get(&1), Some(&(37.0, 91.0, 100.0, 40.0)));
    assert_eq!(hit_test(&m, &rects, (50.0, 100.0)), Some(1));
}

/// `min_size` and `max_size` were stored on every element and read by nobody.
///
/// An element could ask to be "at least 100 wide" and be drawn at whatever
/// width it happened to carry, and nothing would notice: the rectangle the
/// click test uses is the same rectangle the renderer draws.
#[test]
fn min_and_max_size_clamp_the_element() {
    let mut m = HashMap::new();
    let mut small = button(1, [10.0, 10.0, 20.0, 20.0]);
    small.min_size = Some((100.0, 80.0));
    let mut large = button(2, [10.0, 200.0, 400.0, 400.0]);
    large.max_size = Some((120.0, 90.0));
    m.insert(1, small);
    m.insert(2, large);

    let rects = super::super::layout::resolve_rects(&m, (0.0, 0.0, 1920.0, 1080.0));
    assert_eq!(
        rects.get(&1),
        Some(&(10.0, 10.0, 100.0, 80.0)),
        "min_size did not raise the element to its minimum"
    );
    assert_eq!(
        rects.get(&2),
        Some(&(10.0, 200.0, 120.0, 90.0)),
        "max_size did not lower the element to its maximum"
    );

    // The clamp must be what a click is judged against: the clamped edge is
    // inside the element, and the far edge the element asked for is not.
    assert_eq!(
        hit_test(&m, &rects, (95.0, 85.0)),
        Some(1),
        "the clamped area does not accept clicks"
    );
    assert_eq!(
        hit_test(&m, &rects, (115.0, 85.0)),
        None,
        "the element still accepts clicks past the size it was clamped to"
    );
}

/// Constraints were a struct with no entrance: preferred size, fill and aspect
/// were described in the docs and read by nothing.
#[test]
fn constraints_size_the_element() {
    let root = (0.0, 0.0, 400.0, 200.0);

    // Preferred overrides the raw size in both axes.
    let mut preferred = button(1, [0.0, 0.0, 10.0, 10.0]);
    preferred.constraints = UiConstraints {
        preferred_width: Some(150.0),
        preferred_height: Some(30.0),
        ..Default::default()
    };
    assert_eq!(
        super::super::layout::resolve_one(&preferred, root),
        (0.0, 0.0, 150.0, 30.0),
        "preferred_width/height did not size the element"
    );

    // Fill takes a fraction of the containing box, not of the root, and not of
    // the element's own size.
    let mut fill = button(2, [0.0, 0.0, 10.0, 10.0]);
    fill.constraints = UiConstraints::fill();
    assert_eq!(
        super::super::layout::resolve_one(&fill, root),
        (0.0, 0.0, 400.0, 200.0),
        "fill did not take the whole containing box"
    );

    let mut half = button(3, [0.0, 0.0, 10.0, 10.0]);
    half.constraints = UiConstraints::fill_horizontal();
    assert_eq!(
        super::super::layout::resolve_one(&half, root),
        (0.0, 0.0, 400.0, 10.0),
        "fill_horizontal did not take the width and keep the height"
    );

    // The aspect ratio is width/height, so 2.0 is twice as wide as tall. It is
    // applied after fill, to the width that was just decided on.
    let mut wide = button(4, [0.0, 0.0, 100.0, 999.0]);
    wide.constraints = UiConstraints {
        aspect_ratio: Some(2.0),
        ..Default::default()
    };
    assert_eq!(
        super::super::layout::resolve_one(&wide, root),
        (0.0, 0.0, 100.0, 50.0),
        "aspect_ratio did not derive the height from the width"
    );

    // A zero width cannot produce a ratio, and must not produce a negative
    // height: a negative rectangle contains nothing, so the element would be
    // drawn and impossible to click.
    let mut degenerate = button(5, [0.0, 0.0, 0.0, 40.0]);
    degenerate.constraints = UiConstraints {
        aspect_ratio: Some(2.0),
        ..Default::default()
    };
    let rect = super::super::layout::resolve_one(&degenerate, root);
    assert_eq!(rect.2, 0.0, "a zero width was replaced by something else");
    assert!(
        rect.3 > 0.0,
        "aspect_ratio on a zero-width element produced a non-positive height: {rect:?}"
    );
}

/// The clamp and the aspect ratio both change the size the pivot is measured
/// against. This is the order that went wrong when each was added alone.
#[test]
fn the_pivot_is_measured_against_the_settled_size() {
    let mut e = button(1, [0.0, 0.0, 100.0, 100.0]);
    e.pivot = (0.5, 0.5);
    e.anchor = (0.5, 0.5);
    e.min_size = Some((200.0, 100.0));

    // The centre lands on the anchor point of the containing box, (200, 200),
    // and the half-size subtracted to get there is measured from the clamped
    // 200-wide element, not the 100 it asked for. Measuring from the requested
    // size would put the left edge at 150 instead of 100.
    assert_eq!(
        super::super::layout::resolve_one(&e, (0.0, 0.0, 400.0, 400.0)),
        (100.0, 150.0, 200.0, 100.0),
        "the pivot was measured against the pre-clamp size"
    );
}

/// Constraints default to nothing, so an element that never mentions them
/// resolves exactly as it did before they existed. This is the test that keeps
/// the rest of them from being a silent layout change.
#[test]
fn default_constraints_change_nothing() {
    let e = button(1, [37.0, 91.0, 100.0, 40.0]);
    assert_eq!(e.constraints, UiConstraints::default());
    assert_eq!(
        super::super::layout::resolve_one(&e, (0.0, 0.0, 1920.0, 1080.0)),
        (37.0, 91.0, 100.0, 40.0)
    );
}

/// A child is placed against its parent, not against the root.
#[test]
fn a_child_resolves_inside_its_parent() {
    let mut m = HashMap::new();
    let mut parent = button(1, [100.0, 200.0, 400.0, 300.0]);
    parent.interactive = false;
    let child = button(2, [10.0, 20.0, 50.0, 20.0]);
    parent.children.push(2);
    m.insert(1, parent);
    m.insert(2, child);

    let rects = super::super::layout::resolve_rects(&m, (0.0, 0.0, 1920.0, 1080.0));
    assert_eq!(
        rects.get(&2),
        Some(&(110.0, 220.0, 50.0, 20.0)),
        "the child was not placed against its parent"
    );
    assert_eq!(hit_test(&m, &rects, (120.0, 230.0)), Some(2));
    assert_eq!(hit_test(&m, &rects, (20.0, 30.0)), None);
}

/// A tree that points an element at its own ancestor must not hang.
#[test]
fn a_cycle_in_the_tree_terminates() {
    let mut m = HashMap::new();
    let mut a = button(1, [0.0, 0.0, 10.0, 10.0]);
    let mut b = button(2, [0.0, 0.0, 10.0, 10.0]);
    a.children.push(2);
    b.children.push(1);
    m.insert(1, a);
    m.insert(2, b);

    let rects = super::super::layout::resolve_rects(&m, (0.0, 0.0, 100.0, 100.0));
    assert_eq!(rects.len(), 2, "an element in a cycle was dropped or duplicated");
}

#[test]
fn hovering_follows_the_cursor() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();

        update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MouseMoved { x: 50.0, y: 25.0 }],
        );
        assert_eq!(s.hovered, Some(1));
        assert!(m[&1].hovered);

        update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MouseMoved { x: 500.0, y: 500.0 }],
        );
        assert_eq!(s.hovered, None);
        assert!(!m[&1].hovered, "the stale hover flag was left set");
    }

    #[test]
    fn an_absent_cursor_hover_nothing() {
        // Alt-tabbing away parks the cursor somewhere; without this the element
        // under (0,0) stays highlighted forever.
        let mut m = HashMap::new();
        m.insert(1, button(1, [-100.0, -100.0, 200.0, 200.0]));
        let mut s = UiInputState {
            mouse: (0.0, 0.0),
            cursor_present: false,
            ..Default::default()
        };
        let out = update_in(&mut m, &mut s, &[]);
        assert_eq!(out.hovered, None);
    }

    #[test]
    fn press_then_release_over_the_same_element_clicks() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();

        let out = update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::MouseMoved { x: 50.0, y: 25.0 },
                UiInputEvent::MousePressed { x: 50.0, y: 25.0, button: 0 },
            ],
        );
        assert_eq!(out.active, Some(1));
        assert!(m[&1].pressed, "the press flag was not set");
        assert!(out.clicks.is_empty(), "a click fired on the press");

        let out = update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MouseReleased { x: 50.0, y: 25.0, button: 0 }],
        );
        assert_eq!(out.clicks.len(), 1);
        assert_eq!(out.clicks[0].element, 1);
        assert_eq!(out.active, None, "the release left the element active");
        assert!(!m[&1].pressed);
    }

    #[test]
    fn releasing_away_from_the_pressed_element_does_not_click() {
        // This is what makes a drag not fire the button: press on the button,
        // move off, release.
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();

        update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::MouseMoved { x: 50.0, y: 25.0 },
                UiInputEvent::MousePressed { x: 50.0, y: 25.0, button: 0 },
                UiInputEvent::MouseMoved { x: 500.0, y: 500.0 },
            ],
        );
        let out = update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MouseReleased { x: 500.0, y: 500.0, button: 0 }],
        );
        assert!(out.clicks.is_empty(), "a drag fired a click");
        assert_eq!(out.active, None);
    }

    #[test]
    fn the_right_button_does_not_produce_a_click() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();
        update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::MouseMoved { x: 50.0, y: 25.0 },
                UiInputEvent::MousePressed { x: 50.0, y: 25.0, button: 2 },
                UiInputEvent::MouseReleased { x: 50.0, y: 25.0, button: 2 },
            ],
        );
        assert!(
            s.hovered.is_none() || !s.buttons[2],
            "the right button left state behind"
        );
    }

    #[test]
    fn pressing_focuses_the_element_under_the_cursor() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();
        let out = update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MousePressed { x: 50.0, y: 25.0, button: 0 }],
        );
        assert_eq!(out.focused, Some(1));
        assert!(out.focus_moved);
        assert!(m[&1].focused);
    }

    #[test]
    fn tab_moves_focus_forward_and_wraps() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0; 4]));
        m.insert(2, button(2, [0.0; 4]));
        m.insert(3, button(3, [0.0; 4]));
        let mut s = UiInputState::default();

        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(1));
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(2));
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(3));
        // Wraps rather than sticking on the last element.
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(1));
    }

    #[test]
    fn shift_tab_moves_focus_backwards_and_wraps() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0; 4]));
        m.insert(2, button(2, [0.0; 4]));
        let mut s = UiInputState::default();
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(1));
        s.modifiers.shift = true;
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(2), "shift-tab did not go backwards");
        // From the first, backwards wraps to the last.
        s.focused = Some(1);
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(2));
    }

    #[test]
    fn tab_skips_non_focusable_elements() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0; 4]));
        let mut panel = UiElement::panel(2);
        panel.position = (0.0, 0.0);
        panel.size = (10.0, 10.0);
        panel.interactive = true;
        m.insert(2, panel);
        m.insert(3, button(3, [0.0; 4]));

        let mut s = UiInputState::default();
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(1));
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert_eq!(out.focused, Some(3), "focus landed on a panel");
    }

    #[test]
    fn escape_clears_focus() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0; 4]));
        let mut s = UiInputState::default();
        update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Tab }]);
        assert!(s.focused.is_some());
        let out = update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Escape }]);
        assert_eq!(out.focused, None);
        assert!(out.focus_moved);
        assert!(!m[&1].focused);
    }

    #[test]
    fn characters_go_into_the_focused_field() {
        let mut m = HashMap::new();
        m.insert(1, field(1, [0.0, 0.0, 200.0, 30.0], ""));
        let mut s = focused(1);

        let out = update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::CharacterReceived('a'),
                UiInputEvent::CharacterReceived('b'),
            ],
        );
        assert_eq!(out.text, "ab");
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert_eq!(value, "ab");
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn characters_are_ignored_when_no_field_has_focus() {
        let mut m = HashMap::new();
        m.insert(1, field(1, [0.0; 4], "keep"));
        let mut s = UiInputState::default();
        let out = update_in(&mut m, &mut s, &[UiInputEvent::CharacterReceived('z')]);
        assert!(out.text.is_empty());
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert_eq!(value, "keep");
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn characters_are_ignored_on_a_non_text_element() {
        // Focus on a button and typing must not do anything, rather than
        // panicking on a non-InputField payload.
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0; 4]));
        let mut s = focused(1);
        let out = update_in(&mut m, &mut s, &[UiInputEvent::CharacterReceived('z')]);
        assert!(out.text.is_empty());
    }

    #[test]
    fn control_characters_never_reach_a_field() {
        let mut m = HashMap::new();
        m.insert(1, field(1, [0.0; 4], ""));
        let mut s = focused(1);
        update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::CharacterReceived('\n'), UiInputEvent::CharacterReceived('\u{7}')],
        );
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert!(value.is_empty(), "a control character was inserted");
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn backspace_removes_the_last_character() {
        let mut m = HashMap::new();
        m.insert(1, field(1, [0.0; 4], "abc"));
        let mut s = focused(1);
        update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Backspace }]);
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert_eq!(value, "ab");
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn backspace_on_an_empty_field_is_harmless() {
        let mut m = HashMap::new();
        m.insert(1, field(1, [0.0; 4], ""));
        let mut s = focused(1);
        update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Backspace }]);
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert!(value.is_empty());
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn a_field_respects_its_maximum_length() {
        let mut m = HashMap::new();
        let mut e = field(1, [0.0; 4], "");
        if let UiElementData::InputField { max_length, .. } = &mut e.data {
            *max_length = Some(3);
        }
        m.insert(1, e);
        let mut s = focused(1);
        update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::CharacterReceived('a'),
                UiInputEvent::CharacterReceived('b'),
                UiInputEvent::CharacterReceived('c'),
                UiInputEvent::CharacterReceived('d'),
            ],
        );
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert_eq!(value, "abc", "the limit was exceeded");
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn backspace_removes_a_whole_character_not_a_byte() {
        // Truncating by byte would cut a multi-byte character in half and leave
        // invalid UTF-8 in the field.
        let mut m = HashMap::new();
        m.insert(1, field(1, [0.0; 4], "aЖ"));
        let mut s = focused(1);
        update_in(&mut m, &mut s, &[UiInputEvent::KeyPressed { key: UiKey::Backspace }]);
        if let UiElementData::InputField { value, .. } = &m[&1].data {
            assert_eq!(value, "a");
        } else {
            panic!("expected an input field");
        }
    }

    #[test]
    fn scroll_goes_to_the_focused_container() {
        let mut m = HashMap::new();
        let mut e = UiElement::new(UiElementType::ScrollContainer, 1);
        e.interactive = true;
        e.position = (0.0, 0.0);
        e.size = (100.0, 100.0);
        e.data = UiElementData::ScrollContainer {
            scroll_x: 0.0,
            scroll_y: 0.0,
            scroll_width: 100.0,
            scroll_height: 500.0,
        };
        m.insert(1, e);
        let mut s = focused(1);

        let out = update_in(&mut m, &mut s, &[UiInputEvent::Scroll { delta: 3.0 }]);
        assert_eq!(out.scroll, 0.0, "the container did not consume the scroll");
        if let UiElementData::ScrollContainer { scroll_y, .. } = &m[&1].data {
            assert_eq!(*scroll_y, -3.0);
        } else {
            panic!("expected a scroll container");
        }
    }

    #[test]
    fn scroll_with_nothing_focused_stays_unconsumed() {
        let mut m = HashMap::new();
        let mut s = UiInputState::default();
        let out = update_in(&mut m, &mut s, &[UiInputEvent::Scroll { delta: 2.0 }]);
        assert_eq!(out.scroll, 2.0);
    }

    #[test]
    fn a_non_finite_scroll_is_ignored() {
        let mut m = HashMap::new();
        let mut s = UiInputState::default();
        let out = update_in(&mut m, &mut s, &[UiInputEvent::Scroll { delta: f32::NAN }]);
        assert_eq!(out.scroll, 0.0);
    }

    #[test]
    fn releasing_everything_clears_a_held_press() {
        // A press held while the window is deactivated never sees its release,
        // so the element would stay stuck.
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();
        update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::MouseMoved { x: 50.0, y: 25.0 },
                UiInputEvent::MousePressed { x: 50.0, y: 25.0, button: 0 },
            ],
        );
        assert!(s.active.is_some());
        s.release_all();
        assert_eq!(s.active, None);
        assert!(!s.buttons[0]);
        assert!(!s.cursor_present);
    }

    #[test]
    fn a_press_and_release_in_one_frame_still_click() {
        // Input is batched per frame, so a fast click can arrive as one batch.
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();
        let out = update_in(
            &mut m,
            &mut s,
            &[
                UiInputEvent::MousePressed { x: 50.0, y: 25.0, button: 0 },
                UiInputEvent::MouseReleased { x: 50.0, y: 25.0, button: 0 },
            ],
        );
        assert_eq!(out.clicks.len(), 1, "a fast click was dropped");
        assert!(!m[&1].pressed, "the press flag survived the frame");
    }

    #[test]
    fn an_element_that_stops_being_interactive_loses_its_flags() {
        let mut m = HashMap::new();
        m.insert(1, button(1, [0.0, 0.0, 100.0, 50.0]));
        let mut s = UiInputState::default();
        update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MouseMoved { x: 50.0, y: 25.0 }],
        );
        assert!(m[&1].hovered);

        m.get_mut(&1).unwrap().interactive = false;
        update_in(
            &mut m,
            &mut s,
            &[UiInputEvent::MouseMoved { x: 50.0, y: 25.0 }],
        );
        assert_eq!(s.hovered, None);
        assert!(!m[&1].hovered, "the flag was not cleared");
    }
}