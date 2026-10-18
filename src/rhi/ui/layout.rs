//! UI Layout System
//!
//! **CLIENT-SIDE ONLY - NOT CONNECTED TO SERVER CODE**
//!
//! Layout management for UI elements with constraints and anchoring.

use super::UiAlignment;
use super::elements::{UiElement, UiElementId};
use std::collections::{HashMap, HashSet};

/// An absolute rectangle: x, y, width, height.
pub type UiRect = (f32, f32, f32, f32);

/// Places one element inside its containing box.
///
/// Anchor picks the point of the containing box, pivot picks the point of the
/// element itself, and `position` is an offset from there. An element with the
/// default anchor and pivot is unchanged by this function.
///
/// This existed as a module with no entrance: `anchor` and `pivot` were stored
/// on every element and read by nobody, so "right-aligned" and "pivot-centred"
/// were two fields that did nothing. Worse, drawing and hit testing both read
/// `position` directly, so they agreed only because both were wrong - fixing
/// one side alone would have put clicks somewhere other than the pixels.
/// The size an element settles on inside `parent`.
///
/// Split out of [`resolve_one`] because a flow layout has to know how big each
/// item is before it can place any of them: it cannot stack items it has not
/// measured.
pub fn resolved_size(element: &UiElement, parent: UiRect) -> (f32, f32) {
    let (_parent_x, _parent_y, parent_w, parent_h) = parent;
    let c = &element.constraints;
    let (mut w, mut h) = element.size;

    // Preferred size answers "how big should I be", fill answers "how much of
    // the room I am in", and the aspect ratio answers "given the width you just
    // decided on, how tall am I". Each overrides what the previous one chose,
    // which is why they are applied in that order rather than merged.
    if let Some(preferred_w) = c.preferred_width {
        w = preferred_w;
    }
    if let Some(preferred_h) = c.preferred_height {
        h = preferred_h;
    }
    if let Some(fraction) = c.width_fill {
        w = parent_w * fraction;
    }
    if let Some(fraction) = c.height_fill {
        h = parent_h * fraction;
    }
    // Divided rather than multiplied: the field is documented as width/height,
    // so `aspect_ratio: 2.0` means twice as wide as tall. A zero or negative
    // width would make the division produce infinity or a negative height, and
    // a negative height is a rectangle that contains nothing.
    if let Some(aspect) = c.aspect_ratio {
        if aspect > 0.0 && w > 0.0 {
            h = w / aspect;
        }
    }

    // Min and max are honoured before the anchor and the pivot see the size,
    // because both are measured against it: a clamped element must be pivoted
    // around the width it ends up with, not the one it asked for.
    //
    // `min_size` and `max_size` were stored on every element and read by
    // nobody, so an element could ask to be "at least 100 wide" and be drawn at
    // whatever width it happened to carry.
    if let Some((min_w, min_h)) = element.min_size {
        w = w.max(min_w);
        h = h.max(min_h);
    }
    if let Some((max_w, max_h)) = element.max_size {
        w = w.min(max_w);
        h = h.min(max_h);
    }
    // The same clamp a second time, from the constraint side. An element may
    // express its bound either way, and the tighter of the two wins rather than
    // whichever field happened to be checked last.
    if let Some(min_w) = c.min_width {
        w = w.max(min_w);
    }
    if let Some(max_w) = c.max_width {
        w = w.min(max_w);
    }
    if let Some(min_h) = c.min_height {
        h = h.max(min_h);
    }
    if let Some(max_h) = c.max_height {
        h = h.min(max_h);
    }

    (w, h)
}

pub fn resolve_one(element: &UiElement, parent: UiRect) -> UiRect {
    let (parent_x, parent_y, parent_w, parent_h) = parent;
    let (w, h) = resolved_size(element, parent);

    (
        parent_x + element.anchor.0 * parent_w + element.position.0 - element.pivot.0 * w,
        parent_y + element.anchor.1 * parent_h + element.position.1 - element.pivot.1 * h,
        w,
        h,
    )
}

/// Resolves every element's rectangle from the tree, in one pass.
///
/// `root` is the containing box for elements with no parent.
///
/// A malformed tree can point an element at one of its own ancestors. That is
/// not refused, because refusing it would drop elements the caller asked for,
/// but it must not become an infinite loop - hence the visited set.
pub fn resolve_rects(elements: &HashMap<UiElementId, UiElement>, root: UiRect) -> HashMap<UiElementId, UiRect> {
    let mut parent_of: HashMap<UiElementId, UiElementId> = HashMap::new();
    for element in elements.values() {
        for &child in &element.children {
            // An element claimed by two parents keeps the first; drawing it
            // twice at different places would be worse than drawing it once.
            parent_of.entry(child).or_insert(element.id);
        }
    }

    let mut rects: HashMap<UiElementId, UiRect> = HashMap::with_capacity(elements.len());
    let mut visited: HashSet<UiElementId> = HashSet::with_capacity(elements.len());
    let mut stack: Vec<(UiElementId, UiRect)> = Vec::new();

    for element in elements.values() {
        if parent_of.contains_key(&element.id) {
            continue;
        }
        let rect = resolve_one(element, root);
        rects.insert(element.id, rect);
        visited.insert(element.id);
        stack.push((element.id, rect));
    }
    // Children must be placed before anything else is seeded. Seeding first would
    // let a child be placed against the root before its parent was walked.
    drain_subtrees(&mut stack, elements, &mut rects, &mut visited);

    // A cycle means no element has a parent, so the pass above seeds nothing and
    // the whole UI would come back unresolved - drawn from the raw position but
    // impossible to click, because hit testing only accepts a resolved rect.
    // Seed whatever is left instead. A malformed tree should lose its shape,
    // not its interactivity.
    for element in elements.values() {
        if !visited.insert(element.id) {
            continue;
        }
        let rect = resolve_one(element, root);
        rects.insert(element.id, rect);
        stack.push((element.id, rect));
    }
    drain_subtrees(&mut stack, elements, &mut rects, &mut visited);

    rects
}

/// Walks seeded elements depth-first, resolving their children against them.
fn drain_subtrees(
    stack: &mut Vec<(UiElementId, UiRect)>,
    elements: &HashMap<UiElementId, UiElement>,
    rects: &mut HashMap<UiElementId, UiRect>,
    visited: &mut HashSet<UiElementId>,
) {
    while let Some((id, rect)) = stack.pop() {
        let Some(parent) = elements.get(&id) else {
            continue;
        };
        if parent.layout == UiLayoutMode::Absolute {
            for &child_id in &parent.children {
                if !visited.insert(child_id) {
                    continue;
                }
                let Some(child) = elements.get(&child_id) else {
                    continue;
                };
                let child_rect = resolve_one(child, rect);
                rects.insert(child_id, child_rect);
                stack.push((child_id, child_rect));
            }
            continue;
        }

        // Flow layout: every child is measured first, then placed in order.
        // A child cannot be placed until its size is known, which is exactly
        // why the measurement lives in `resolved_size` and not in `resolve_one`.
        let [pad_l, pad_t, pad_r, pad_b] = parent.style.padding;
        let [mar_l, mar_t, mar_r, mar_b] = parent.style.margin;
        let layout = UiLayout::new(rect.2, rect.3)
            .with_padding(pad_l, pad_t, pad_r, pad_b)
            .with_margin(mar_l, mar_t, mar_r, mar_b)
            .with_spacing(parent.layout_spacing);
        let children: Vec<&UiElement> =
            parent.children.iter().filter_map(|c| elements.get(c)).collect();
        let sizes: Vec<(f32, f32)> = children.iter().map(|c| resolved_size(c, rect)).collect();
        let placed = if parent.layout == UiLayoutMode::Row {
            layout.place_row(&sizes)
        } else {
            layout.place_column(&sizes)
        };
        for (child, local) in children.into_iter().zip(placed) {
            if !visited.insert(child.id) {
                continue;
            }
            let child_rect = (rect.0 + local.0, rect.1 + local.1, local.2, local.3);
            rects.insert(child.id, child_rect);
            stack.push((child.id, child_rect));
        }
    }
}

/// Whether a point falls inside a resolved rectangle.
///
/// Kept here next to [`resolve_rects`] so the two cannot disagree about where
/// an element is. An invisible or disabled element contains nothing.
pub fn rect_contains(element: &UiElement, rect: UiRect, x: f32, y: f32) -> bool {
    if !element.style.visible || !element.enabled {
        return false;
    }
    let (rx, ry, w, h) = rect;
    // NaN fails every comparison below, so it correctly contains nothing.
    x >= rx && x <= rx + w && y >= ry && y <= ry + h
}

/// Layout result for an element
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UiLayoutResult {
    /// Final x position
    pub x: f32,
    /// Final y position
    pub y: f32,
    /// Final width
    pub width: f32,
    /// Final height
    pub height: f32,
}

/// Layout constraints for UI elements
#[derive(Debug, Clone, Copy, PartialEq)]
#[derive(Default)]
pub struct UiConstraints {
    /// Minimum width
    pub min_width: Option<f32>,
    /// Maximum width
    pub max_width: Option<f32>,
    /// Preferred width
    pub preferred_width: Option<f32>,
    /// Minimum height
    pub min_height: Option<f32>,
    /// Maximum height
    pub max_height: Option<f32>,
    /// Preferred height
    pub preferred_height: Option<f32>,
    /// Width fill (1.0 = fill parent)
    pub width_fill: Option<f32>,
    /// Height fill (1.0 = fill parent)
    pub height_fill: Option<f32>,
    /// Aspect ratio (width / height)
    pub aspect_ratio: Option<f32>,
}


impl UiConstraints {
    /// Create constraints with minimum size
    pub fn min_size(width: f32, height: f32) -> Self {
        Self {
            min_width: Some(width),
            min_height: Some(height),
            ..Default::default()
        }
    }

    /// Create constraints with exact size
    pub fn exact_size(width: f32, height: f32) -> Self {
        Self {
            min_width: Some(width),
            max_width: Some(width),
            min_height: Some(height),
            max_height: Some(height),
            ..Default::default()
        }
    }

    /// Create constraints with maximum size
    pub fn max_size(width: f32, height: f32) -> Self {
        Self {
            max_width: Some(width),
            max_height: Some(height),
            ..Default::default()
        }
    }

    /// Create fill constraints
    pub fn fill() -> Self {
        Self {
            width_fill: Some(1.0),
            height_fill: Some(1.0),
            ..Default::default()
        }
    }

    /// Create horizontal fill constraints
    pub fn fill_horizontal() -> Self {
        Self {
            width_fill: Some(1.0),
            ..Default::default()
        }
    }

    /// Create vertical fill constraints
    pub fn fill_vertical() -> Self {
        Self {
            height_fill: Some(1.0),
            ..Default::default()
        }
    }
}

/// Anchor point for layout
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct UiAnchor {
    /// Horizontal anchor (0.0 = left, 0.5 = center, 1.0 = right)
    pub x: f32,
    /// Vertical anchor (0.0 = top, 0.5 = center, 1.0 = bottom)
    pub y: f32,
}

impl UiAnchor {
    /// Create anchor from alignment
    pub fn from_alignment(alignment: UiAlignment) -> Self {
        let (x, y) = alignment.to_anchor();
        Self { x, y }
    }
    
    /// Top-left anchor
    pub fn top_left() -> Self {
        Self { x: 0.0, y: 0.0 }
    }
    
    /// Top-center anchor
    pub fn top_center() -> Self {
        Self { x: 0.5, y: 0.0 }
    }
    
    /// Top-right anchor
    pub fn top_right() -> Self {
        Self { x: 1.0, y: 0.0 }
    }
    
    /// Center-left anchor
    pub fn center_left() -> Self {
        Self { x: 0.0, y: 0.5 }
    }
    
    /// Center anchor
    pub fn center() -> Self {
        Self { x: 0.5, y: 0.5 }
    }
    
    /// Center-right anchor
    pub fn center_right() -> Self {
        Self { x: 1.0, y: 0.5 }
    }
    
    /// Bottom-left anchor
    pub fn bottom_left() -> Self {
        Self { x: 0.0, y: 1.0 }
    }
    
    /// Bottom-center anchor
    pub fn bottom_center() -> Self {
        Self { x: 0.5, y: 1.0 }
    }
    
    /// Bottom-right anchor
    pub fn bottom_right() -> Self {
        Self { x: 1.0, y: 1.0 }
    }
}

/// Layout manager for UI
pub struct UiLayout {
    /// Parent dimensions
    parent_width: f32,
    parent_height: f32,
    /// Padding
    padding: [f32; 4],
    /// Margin
    margin: [f32; 4],
    /// Spacing between elements
    spacing: f32,
}

impl Default for UiLayout {
    fn default() -> Self {
        Self {
            parent_width: 0.0,
            parent_height: 0.0,
            padding: [0.0, 0.0, 0.0, 0.0],
            margin: [0.0, 0.0, 0.0, 0.0],
            spacing: 0.0,
        }
    }
}

impl UiLayout {
    /// Create a new layout with parent dimensions
    pub fn new(parent_width: f32, parent_height: f32) -> Self {
        Self {
            parent_width,
            parent_height,
            ..Default::default()
        }
    }
    
    /// Set padding
    pub fn with_padding(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        self.padding = [left, top, right, bottom];
        self
    }
    
    /// Set uniform padding
    pub fn with_uniform_padding(mut self, padding: f32) -> Self {
        self.padding = [padding, padding, padding, padding];
        self
    }
    
    /// Set margin
    pub fn with_margin(mut self, left: f32, top: f32, right: f32, bottom: f32) -> Self {
        self.margin = [left, top, right, bottom];
        self
    }
    
    /// Set spacing
    pub fn with_spacing(mut self, spacing: f32) -> Self {
        self.spacing = spacing;
        self
    }

    /// The box the children are placed in: the parent inset by the padding.
    ///
    /// Coordinates are relative to the parent's top-left corner, so the caller
    /// adds the parent's own origin. The size saturates at zero rather than
    /// going negative, because padding larger than the parent is a request to
    /// show nothing, not a rectangle that contains everything.
    pub fn content_box(&self) -> UiRect {
        let [left, top, right, bottom] = self.padding;
        (
            left,
            top,
            (self.parent_width - left - right).max(0.0),
            (self.parent_height - top - bottom).max(0.0),
        )
    }

    /// Stacks `sizes` top to bottom inside the padded content box.
    ///
    /// Each item is inset by the margin on all four sides, and the next item
    /// starts `spacing` below the previous item's bottom margin. A flow places
    /// children in one direction, so a per-child margin is deliberately not
    /// consulted here: it would silently change the row pitch.
    pub fn place_column(&self, sizes: &[(f32, f32)]) -> Vec<UiRect> {
        let (x, mut y, _w, _h) = self.content_box();
        let [m_left, m_top, _m_right, m_bottom] = self.margin;
        let mut out = Vec::with_capacity(sizes.len());
        for (index, &(w, h)) in sizes.iter().enumerate() {
            if index > 0 {
                y += self.spacing;
            }
            out.push((x + m_left, y + m_top, w, h));
            y += m_top + h + m_bottom;
        }
        out
    }

    /// Stacks `sizes` left to right inside the padded content box.
    pub fn place_row(&self, sizes: &[(f32, f32)]) -> Vec<UiRect> {
        let (mut x, y, _w, _h) = self.content_box();
        let [m_left, m_top, m_right, _m_bottom] = self.margin;
        let mut out = Vec::with_capacity(sizes.len());
        for (index, &(w, h)) in sizes.iter().enumerate() {
            if index > 0 {
                x += self.spacing;
            }
            out.push((x + m_left, y + m_top, w, h));
            x += m_left + w + m_right;
        }
        out
    }
}

/// How a container arranges its children.
///
/// `Absolute` is the default and the behaviour that predates flow layout: a
/// child is placed by its own anchor, pivot and position. `Row` and `Column`
/// ignore those and stack children in order, which is what a toolbar, menu or
/// settings list actually wants - and what [`UiLayout`] was written for while
/// nothing ever called it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UiLayoutMode {
    /// Children keep their own anchor, pivot and position.
    #[default]
    Absolute,
    /// Children are stacked left to right.
    Row,
    /// Children are stacked top to bottom.
    Column,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(container: UiElement, children: Vec<UiElement>) -> HashMap<UiElementId, UiElement> {
        let mut map = HashMap::new();
        for child in children {
            map.insert(child.id, child);
        }
        map.insert(container.id, container);
        map
    }

    #[test]
    fn content_box_insets_by_padding() {
        let layout = UiLayout::new(100.0, 100.0).with_padding(10.0, 20.0, 30.0, 40.0);
        assert_eq!(layout.content_box(), (10.0, 20.0, 60.0, 40.0));
    }

    #[test]
    fn content_box_saturates_at_zero() {
        let layout = UiLayout::new(10.0, 10.0).with_padding(20.0, 20.0, 20.0, 20.0);
        assert_eq!(layout.content_box(), (20.0, 20.0, 0.0, 0.0));
    }

    #[test]
    fn a_column_stacks_with_spacing_and_margin() {
        let layout = UiLayout::new(200.0, 200.0)
            .with_padding(5.0, 5.0, 5.0, 5.0)
            .with_margin(1.0, 2.0, 3.0, 4.0)
            .with_spacing(4.0);
        let placed = layout.place_column(&[(50.0, 10.0), (50.0, 20.0)]);
        assert_eq!(placed[0], (6.0, 7.0, 50.0, 10.0));
        assert_eq!(placed[1], (6.0, 27.0, 50.0, 20.0));
    }

    #[test]
    fn a_row_stacks_with_spacing_and_margin() {
        let layout = UiLayout::new(200.0, 200.0)
            .with_padding(5.0, 5.0, 5.0, 5.0)
            .with_margin(1.0, 2.0, 3.0, 4.0)
            .with_spacing(4.0);
        let placed = layout.place_row(&[(50.0, 10.0), (20.0, 10.0)]);
        assert_eq!(placed[0], (6.0, 7.0, 50.0, 10.0));
        assert_eq!(placed[1], (64.0, 7.0, 20.0, 10.0));
    }

    #[test]
    fn a_row_container_places_its_children() {
        let mut parent = UiElement::container(1);
        parent.size = (200.0, 100.0);
        parent.layout = UiLayoutMode::Row;
        parent.layout_spacing = 4.0;
        parent.style.padding = [5.0, 5.0, 5.0, 5.0];
        parent.children = vec![2, 3];

        let mut a = UiElement::container(2);
        a.size = (50.0, 10.0);
        let mut b = UiElement::container(3);
        b.size = (20.0, 30.0);

        let map = tree(parent, vec![a, b]);
        let rects = resolve_rects(&map, (0.0, 0.0, 200.0, 100.0));

        assert_eq!(rects[&2], (5.0, 5.0, 50.0, 10.0));
        assert_eq!(rects[&3], (59.0, 5.0, 20.0, 30.0));
    }

    #[test]
    fn a_column_without_spacing_still_stacks() {
        let mut parent = UiElement::container(1);
        parent.size = (100.0, 100.0);
        parent.layout = UiLayoutMode::Column;
        parent.children = vec![2, 3];

        let mut a = UiElement::container(2);
        a.size = (100.0, 20.0);
        let mut b = UiElement::container(3);
        b.size = (100.0, 20.0);

        let map = tree(parent, vec![a, b]);
        let rects = resolve_rects(&map, (0.0, 0.0, 100.0, 100.0));

        assert_eq!(rects[&2], (0.0, 0.0, 100.0, 20.0));
        assert_eq!(rects[&3], (0.0, 20.0, 100.0, 20.0));
    }

    #[test]
    fn an_absolute_container_is_unchanged_by_the_new_fields() {
        let mut parent = UiElement::container(1);
        parent.size = (200.0, 100.0);
        parent.children = vec![2];

        let mut child = UiElement::container(2);
        child.size = (50.0, 10.0);
        child.position = (7.0, 9.0);

        let map = tree(parent, vec![child]);
        let rects = resolve_rects(&map, (0.0, 0.0, 200.0, 100.0));

        assert_eq!(rects[&2], (7.0, 9.0, 50.0, 10.0));
    }
}
