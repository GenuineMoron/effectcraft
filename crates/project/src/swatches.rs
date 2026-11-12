//! Project swatches: named colours that colour properties link to (#3362).
//!
//! A swatch belongs to the project. A colour property links to it by the swatch's id
//! ([`Property::swatch`](crate::Property::swatch)), so renaming a swatch keeps its links, and
//! duplicating or copying a linked layer keeps the link. A linked property reads the swatch's
//! colour wherever values are evaluated (keyframes are ignored while linked; an expression still
//! runs on top of it), so changing a swatch recolours everything linked to it.
//!
//! A swatch can have a colour per **theme** (`Dark`, a sponsor's colours…). The project's active
//! theme picks those; a swatch without a colour for the active theme uses its own. Switching the
//! theme recolours every linked property of the project at once.
//!
//! A **solid** links by its item ([`Swatches::solids`]): after every edit [`sync_solids`] writes the
//! swatch's colour into the solid itself, so everything that reads a solid's colour (the renderer,
//! Solid Settings, exports) shows the linked colour with no change of its own.
//!
//! Colours are stored like colour property values (RGBA 0..1 in the project's working space).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// A swatch's id, unique in the project (allocated from [`Project::next_id`](crate::Project)).
pub type SwatchId = u64;

/// One named colour.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Swatch {
    pub id: SwatchId,
    pub name: String,
    /// The palette it belongs to (`Brand`, `Event 2026`…); empty for none.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub group: String,
    /// The colour (RGBA 0..1).
    pub color: [f64; 4],
    /// Its colour in other themes, by theme name.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub themes: BTreeMap<String, [f64; 4]>,
}

impl Swatch {
    /// The colour in `theme` (its own colour when it has none there, or for `None`).
    pub fn color_in(&self, theme: Option<&str>) -> [f64; 4] {
        theme.and_then(|t| self.themes.get(t)).copied().unwrap_or(self.color)
    }
}

/// The project's swatches and its active theme.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Swatches {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub list: Vec<Swatch>,
    /// The active theme; `None` is every swatch's own colour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub theme: Option<String>,
    /// Solids linked to a swatch, by solid item id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub solids: BTreeMap<u64, SwatchId>,
}

impl Swatches {
    pub fn is_empty(&self) -> bool {
        self.list.is_empty() && self.theme.is_none() && self.solids.is_empty()
    }
    pub fn get(&self, id: SwatchId) -> Option<&Swatch> {
        self.list.iter().find(|s| s.id == id)
    }
    pub fn get_mut(&mut self, id: SwatchId) -> Option<&mut Swatch> {
        self.list.iter_mut().find(|s| s.id == id)
    }
    /// A swatch by name (exact, then ignoring case).
    pub fn by_name(&self, name: &str) -> Option<&Swatch> {
        self.list.iter().find(|s| s.name == name).or_else(|| self.list.iter().find(|s| s.name.eq_ignore_ascii_case(name)))
    }
    /// The colour a property linked to `id` shows in the active theme, or `None` when the swatch
    /// no longer exists (the property then shows its own value).
    pub fn resolve(&self, id: SwatchId) -> Option<[f64; 4]> {
        self.get(id).map(|s| s.color_in(self.theme.as_deref()))
    }
    /// Every theme any swatch has a colour for, sorted.
    pub fn themes(&self) -> Vec<String> {
        let mut t: Vec<String> = self.list.iter().flat_map(|s| s.themes.keys().cloned()).collect();
        t.sort();
        t.dedup();
        t
    }
    /// A hash of everything that changes what linked properties show (render cache keys).
    pub fn content_hash(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.theme.hash(&mut h);
        for s in &self.list {
            s.id.hash(&mut h);
            for c in s.color_in(self.theme.as_deref()) {
                c.to_bits().hash(&mut h);
            }
        }
        h.finish()
    }
}

/// After an edit: give every solid linked to a swatch that swatch's colour in the active theme. A
/// link whose solid or swatch is gone is dropped (the solid keeps its colour).
pub fn sync_solids(project: &mut crate::Project) {
    if project.swatches.solids.is_empty() {
        return;
    }
    let links: Vec<(u64, SwatchId)> = project.swatches.solids.iter().map(|(i, s)| (*i, *s)).collect();
    for (item, id) in links {
        let color = project.swatches.resolve(id);
        let solid = match project.items.get_mut(&crate::ItemId(item)).map(|it| &mut it.kind) {
            Some(crate::ItemKind::Solid(solid)) => Some(solid),
            _ => None,
        };
        match (solid, color) {
            (Some(solid), Some(c)) => solid.color = [c[0] as f32, c[1] as f32, c[2] as f32],
            _ => {
                project.swatches.solids.remove(&item);
            }
        }
    }
}
