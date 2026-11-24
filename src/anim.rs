//! NSH's native animation graph ([`Animator`]), run beside the engine: the
//! Mini Program's frames tick it, scroll offsets feed it, and each bound
//! view gets its animated properties as inline CSS (`["a", id, css]`),
//! which the tree adds after the view's style.

use std::{collections::HashMap, fmt::Write as _};

use nsh_core::{
    MountItem, Transform,
    animation::{AnimValues, Animator, Finished},
    geometry::{Point, Points},
    time::Instant,
};
use serde_json::{Value, json};

use crate::{effects, encode::color};

/// The graph and what it last wrote.
pub(crate) struct Anim {
    animator: Animator,
    /// The clock's zero (the graph counts seconds).
    start: Instant,
    /// Each view's static transform, whose animated entries the graph
    /// replaces.
    statics: HashMap<u32, Transform>,
    /// The CSS each bound view was last given.
    written: HashMap<u32, String>,
}

impl Default for Anim {
    fn default() -> Self {
        Anim {
            animator: Animator::default(),
            start: nsh_core::time::now(),
            statics: HashMap::new(),
            written: HashMap::new(),
        }
    }
}

impl Anim {
    /// Looks at one item of a batch.
    pub(crate) fn apply(&mut self, item: &MountItem) {
        match item {
            MountItem::ViewStyle { id, style } => {
                self.statics.insert(*id, style.transform);
            }
            MountItem::Delete { id } => {
                self.statics.remove(id);
                self.written.remove(id);
            }
            _ => {}
        }
        self.animator.apply(item);
    }

    /// Advances running animations to now; true while one runs.
    pub(crate) fn tick(&mut self) -> bool {
        let now = self.start.elapsed().as_secs_f64();
        self.animator.tick(now)
    }

    /// Whether an animation runs (the next frame is wanted).
    pub(crate) fn animating(&self) -> bool {
        self.animator.animating()
    }

    /// Scroll view `id` is at `offset`: true if a value follows it.
    pub(crate) fn scrolled(&mut self, id: u32, offset: Point<Points>) -> bool {
        self.animator.scrolled(id, offset)
    }

    /// Animations that finished since the last call.
    pub(crate) fn finished(&mut self) -> Vec<Finished> {
        std::mem::take(&mut self.animator.finished)
    }

    /// The `["a", id, css]` operations of the views whose animated values
    /// changed since the last call (`""` for views bound no more).
    pub(crate) fn outputs(&mut self) -> Vec<Value> {
        let mut now = HashMap::new();
        let statics = &self.statics;
        self.animator
            .write(Some, |id, values: AnimValues, _canvas| {
                now.insert(id, css(&values, statics.get(&id)));
            });
        let mut ops = Vec::new();
        for (id, css) in &now {
            if self.written.get(id) != Some(css) {
                ops.push(json!(["a", id, css]));
            }
        }
        for id in self.written.keys() {
            if !now.contains_key(id) {
                ops.push(json!(["a", id, ""]));
            }
        }
        self.written = now;
        ops
    }
}

fn css(values: &AnimValues, statics: Option<&Transform>) -> String {
    let mut out = String::new();
    if let Some(o) = values.opacity {
        let _ = write!(out, "opacity:{};", (o * 1000.0).round() / 1000.0);
    }
    if let Some(c) = values.background {
        let _ = write!(out, "background-color:{};", color(c));
    }
    if let Some(c) = values.border_color {
        let _ = write!(out, "border-color:{};", color(c));
    }
    if values.transform.iter().any(Option::is_some) {
        let base = statics.copied().unwrap_or(Transform::IDENTITY);
        let t = effects::transform(&base, |i| {
            values.transform[i].map(|v| (v * 100.0).round() / 100.0)
        });
        let _ = write!(out, "transform:{t};");
    }
    out
}
