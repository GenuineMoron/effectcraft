//! Project swatches (#3362): named colours that colour properties link to, with themes
//! (`effectcraft_project::swatches`). A linked property shows its swatch's colour in the active
//! theme; editing the swatch or switching the theme recolours everything linked to it.

use effectcraft_keyframe::Value as KV;
use effectcraft_project::swatches::{Swatch, SwatchId};
use effectcraft_project::{Project, Uid};
use serde_json::{Value, json};

use super::prop::prop_ref;
use super::{CommandSpec, always, bad, has_comp, str_p};
use crate::{Result, Session, cmd, query};

/// A colour parameter: `[r, g, b]` or `[r, g, b, a]` (0..1) or `#rrggbb`. Opaque when no alpha.
fn color_p(p: &Value, k: &str, c: &str) -> Result<Option<[f64; 4]>> {
    let Some(v) = p.get(k) else { return Ok(None) };
    let bad_color = || bad(c, format!("`{k}`: a colour [r, g, b(, a)] (0..1) or \"#rrggbb\""));
    if let Some(s) = v.as_str() {
        let h = s.trim().trim_start_matches('#');
        if h.len() != 6 || !h.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return Err(bad_color());
        }
        let byte = |i: usize| h.get(i..i + 2).and_then(|b| u8::from_str_radix(b, 16).ok()).map(|b| f64::from(b) / 255.0);
        return match (byte(0), byte(2), byte(4)) {
            (Some(r), Some(g), Some(b)) => Ok(Some([r, g, b, 1.0])),
            _ => Err(bad_color()),
        };
    }
    let a = v.as_array().ok_or_else(bad_color)?;
    let n: Vec<f64> = a.iter().filter_map(Value::as_f64).collect();
    if n.len() != a.len() || !(3..=4).contains(&n.len()) || n.iter().any(|x| !x.is_finite()) {
        return Err(bad_color());
    }
    let ch = |i: usize| n.get(i).copied().unwrap_or(1.0);
    Ok(Some([ch(0), ch(1), ch(2), ch(3)]))
}

fn hex(c: [f64; 4]) -> String {
    let b = |x: f64| (x.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02x}{:02x}{:02x}", b(c[0]), b(c[1]), b(c[2]))
}

/// The swatch a `swatch` parameter names: its id, or its name.
fn swatch_p(project: &Project, p: &Value, c: &str) -> Result<SwatchId> {
    let sw = &project.swatches;
    match p.get("swatch") {
        Some(Value::Number(n)) => n.as_u64().filter(|id| sw.get(*id).is_some()).ok_or_else(|| bad(c, format!("no swatch {n}"))),
        Some(Value::String(name)) => sw.by_name(name).map(|s| s.id).ok_or_else(|| bad(c, format!("no swatch named `{name}`"))),
        _ => Err(bad(c, "missing `swatch` (id or name)")),
    }
}

/// Every linked colour property: (comp, layer, property uid, property name, swatch).
fn links(project: &Project) -> Vec<(u64, u64, Uid, String, SwatchId)> {
    let mut out = Vec::new();
    for (cid, item) in &project.items {
        let effectcraft_project::ItemKind::Comp(comp) = &item.kind else { continue };
        for l in comp.layers.iter() {
            l.props.walk("", &mut |_, pr| {
                if let Some(id) = pr.swatch {
                    out.push((cid.0, l.id.0, pr.uid, pr.name.clone(), id));
                }
            });
        }
    }
    out
}

fn swatch_json(s: &Swatch, theme: Option<&str>, uses: usize) -> Value {
    let shown = s.color_in(theme);
    json!({"id": s.id, "name": s.name, "group": s.group, "color": s.color, "hex": hex(s.color), "shown": shown, "shownHex": hex(shown),
           "themes": s.themes, "uses": uses})
}

fn list(s: &mut Session, _: &Value) -> Result<Value> {
    let sw = &s.project.swatches;
    let used = links(&s.project);
    let swatches: Vec<Value> = sw.list.iter().map(|x| swatch_json(x, sw.theme.as_deref(), used.iter().filter(|u| u.4 == x.id).count())).collect();
    Ok(json!({"theme": sw.theme, "themes": sw.themes(), "swatches": swatches}))
}

fn new(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.new";
    let name = str_p(p, "name").map(str::trim).filter(|n| !n.is_empty()).ok_or_else(|| bad(c, "missing `name`"))?.to_string();
    let color = color_p(p, "color", c)?.ok_or_else(|| bad(c, "missing `color`"))?;
    let group = str_p(p, "group").unwrap_or("").to_string();
    if s.project.swatches.list.iter().any(|x| x.name == name) {
        return Err(bad(c, format!("a swatch named `{name}` exists")));
    }
    let id = s.edit("New Swatch", None, |proj, _| {
        let id = proj.next_id;
        proj.next_id = proj.next_id.saturating_add(1);
        proj.swatches.list.push(Swatch { id, name, group, color, themes: Default::default() });
        Ok(id)
    })?;
    Ok(json!({"swatch": id}))
}

fn edit(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.edit";
    let id = swatch_p(&s.project, p, c)?;
    let name = str_p(p, "name").map(str::trim).map(str::to_string);
    if name.as_deref().is_some_and(str::is_empty) {
        return Err(bad(c, "`name` is empty"));
    }
    if let Some(n) = &name
        && s.project.swatches.list.iter().any(|x| x.id != id && &x.name == n)
    {
        return Err(bad(c, format!("a swatch named `{n}` exists")));
    }
    let color = color_p(p, "color", c)?;
    let group = str_p(p, "group").map(str::to_string);
    let theme = str_p(p, "theme").map(str::trim).map(str::to_string);
    let theme_color = color_p(p, "themeColor", c)?;
    if theme_color.is_some() && theme.as_deref().is_none_or(str::is_empty) {
        return Err(bad(c, "`themeColor` needs `theme`"));
    }
    let remove_theme = p.get("themeColor").is_some_and(Value::is_null) && theme.is_some();
    s.edit("Edit Swatch", super::merge_p(p), |proj, _| {
        let sw = proj.swatches.get_mut(id).ok_or_else(|| bad(c, "the swatch no longer exists"))?;
        if let Some(n) = name {
            sw.name = n;
        }
        if let Some(col) = color {
            sw.color = col;
        }
        if let Some(g) = group {
            sw.group = g;
        }
        match (theme, theme_color) {
            (Some(t), Some(col)) => {
                sw.themes.insert(t, col);
            }
            (Some(t), None) if remove_theme => {
                sw.themes.remove(&t);
            }
            _ => {}
        }
        Ok(())
    })?;
    let sw = &s.project.swatches;
    let x = sw.get(id).ok_or_else(|| bad(c, "the swatch no longer exists"))?;
    Ok(swatch_json(x, sw.theme.as_deref(), links(&s.project).iter().filter(|u| u.4 == id).count()))
}

/// Delete a swatch. Its linked properties keep the colour they show, as their own value.
fn delete(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.delete";
    let id = swatch_p(&s.project, p, c)?;
    let n = s.edit("Delete Swatch", None, |proj, _| {
        let shown = proj.swatches.resolve(id);
        let n = unlink_all(proj, id, shown);
        proj.swatches.list.retain(|x| x.id != id);
        Ok(n)
    })?;
    Ok(json!({"unlinked": n}))
}

/// Unlink every property linked to `id`, giving each `keep` (the colour it showed) as its value.
fn unlink_all(proj: &mut Project, id: SwatchId, keep: Option<[f64; 4]>) -> usize {
    let mut n = 0;
    let comps: Vec<_> = proj.items.keys().copied().collect();
    for cid in comps {
        let Some(comp) = proj.comp_mut(cid) else { continue };
        for l in comp.layers.iter_mut() {
            l.props.walk_mut(&mut |pr| {
                if pr.swatch == Some(id) {
                    pr.swatch = None;
                    if let Some(k) = keep {
                        pr.value = KV::Color(k);
                    }
                    n += 1;
                }
            });
        }
    }
    n
}

fn link(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.link";
    let (cid, lid, uid) = prop_ref(s, p, c)?;
    let id = swatch_p(&s.project, p, c)?;
    s.edit("Link to Swatch", None, |proj, _| {
        let l = super::layer_mut(proj, cid, lid)?;
        let pr = l.props.find_mut(uid).ok_or_else(|| bad(c, "the property no longer exists"))?;
        if !matches!(pr.value, KV::Color(_)) {
            return Err(bad(c, format!("`{}` is not a colour property", pr.name)));
        }
        // Keyframes stay (unlinking shows them again) but don't apply while linked.
        pr.swatch = Some(id);
        Ok(())
    })?;
    Ok(json!({"layer": lid.0, "prop": uid, "swatch": id}))
}

/// Unlink a property: it keeps the colour it showed as its own value (keyframes, if any, apply again).
fn unlink(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.unlink";
    let (cid, lid, uid) = prop_ref(s, p, c)?;
    s.edit("Unlink Swatch", None, |proj, _| {
        let shown = {
            let l = proj.comp(cid).and_then(|comp| comp.layer(lid)).ok_or_else(|| bad(c, "no layer"))?;
            l.props.find(uid).and_then(|pr| pr.swatch).and_then(|id| proj.swatches.resolve(id))
        };
        let l = super::layer_mut(proj, cid, lid)?;
        let pr = l.props.find_mut(uid).ok_or_else(|| bad(c, "the property no longer exists"))?;
        if pr.swatch.take().is_none() {
            return Err(bad(c, format!("`{}` is not linked to a swatch", pr.name)));
        }
        if let Some(k) = shown {
            pr.value = KV::Color(k);
        }
        Ok(())
    })?;
    Ok(json!({"layer": lid.0, "prop": uid}))
}

fn theme(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.theme";
    let t = match p.get("theme") {
        None | Some(Value::Null) => None,
        Some(Value::String(t)) if t.trim().is_empty() => None,
        Some(Value::String(t)) => Some(t.trim().to_string()),
        _ => return Err(bad(c, "`theme`: a theme name, or null for the swatches' own colours")),
    };
    s.edit("Swatch Theme", None, |proj, _| {
        proj.swatches.theme = t;
        Ok(())
    })?;
    Ok(json!({"theme": s.project.swatches.theme, "themes": s.project.swatches.themes()}))
}

fn usage(s: &mut Session, p: &Value) -> Result<Value> {
    let id = swatch_p(&s.project, p, "swatch.usage")?;
    let uses: Vec<Value> = links(&s.project)
        .into_iter()
        .filter(|u| u.4 == id)
        .map(|(comp, layer, prop, name, _)| json!({"comp": comp, "layer": layer, "prop": prop, "name": name}))
        .collect();
    let solids: Vec<Value> = s
        .project
        .swatches
        .solids
        .iter()
        .filter(|(_, sw)| **sw == id)
        .map(|(item, _)| json!({"item": item, "name": s.project.item(effectcraft_project::ItemId(*item)).map(|i| i.name.clone())}))
        .collect();
    Ok(json!({"swatch": id, "uses": uses, "solids": solids}))
}

/// The solid an `item` parameter names: its id, or its name.
fn solid_p(project: &Project, p: &Value, c: &str) -> Result<u64> {
    let is_solid = |it: &effectcraft_project::Item| matches!(it.kind, effectcraft_project::ItemKind::Solid(_));
    let found = match p.get("item") {
        Some(Value::Number(n)) => n.as_u64().filter(|id| project.item(effectcraft_project::ItemId(*id)).is_some_and(is_solid)),
        Some(Value::String(name)) => project.items.iter().find(|(_, it)| &it.name == name && is_solid(it)).map(|(id, _)| id.0),
        _ => return Err(bad(c, "missing `item` (a solid's id or name)")),
    };
    found.ok_or_else(|| bad(c, "`item` is not a solid of the project"))
}

/// Link a solid to a swatch: it takes the swatch's colour now and whenever the swatch or the
/// theme changes (every layer using the solid follows).
fn link_solid(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.linkSolid";
    let item = solid_p(&s.project, p, c)?;
    let id = swatch_p(&s.project, p, c)?;
    s.edit("Link Solid to Swatch", None, |proj, _| {
        proj.swatches.solids.insert(item, id);
        Ok(())
    })?;
    Ok(json!({"item": item, "swatch": id}))
}

/// Unlink a solid: it keeps the colour it has.
fn unlink_solid(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.unlinkSolid";
    let item = solid_p(&s.project, p, c)?;
    if !s.project.swatches.solids.contains_key(&item) {
        return Err(bad(c, "the solid is not linked to a swatch"));
    }
    s.edit("Unlink Solid", None, |proj, _| {
        proj.swatches.solids.remove(&item);
        Ok(())
    })?;
    Ok(json!({"item": item}))
}

/// Link every colour property and solid whose own colour matches a swatch: how a project made
/// with fixed colours (or a palette comp read by expressions) adopts its brand swatches.
/// Properties already linked, keyed or driven by an expression are left alone, and so are the
/// solids of adjustment layers; with `comp`, only that comp's properties (no solids). `tolerance` is per channel (0..1,
/// default half a level of 8 bits); `dryRun` only counts.
fn adopt(s: &mut Session, p: &Value) -> Result<Value> {
    let c = "swatch.adopt";
    let tol = super::f_p(p, "tolerance").unwrap_or(0.5 / 255.0);
    if !(0.0..=1.0).contains(&tol) {
        return Err(bad(c, "`tolerance`: 0..1 per channel"));
    }
    let dry = super::b_p(p, "dryRun").unwrap_or(false);
    let only: Option<u64> = match p.get("comp") {
        None | Some(Value::Null) => None,
        Some(_) => Some(super::comp_id(s, p)?.0),
    };
    let swatches: Vec<(SwatchId, [f64; 4])> = s.project.swatches.list.iter().map(|x| (x.id, x.color)).collect();
    if swatches.is_empty() {
        return Err(bad(c, "the project has no swatches"));
    }
    let matches = |v: &KV| -> Option<SwatchId> {
        let KV::Color(col) = v else { return None };
        swatches.iter().find(|(_, sc)| (0..3).all(|i| (col[i] - sc[i]).abs() <= tol)).map(|(id, _)| *id)
    };
    let mut found: Vec<(effectcraft_project::ItemId, effectcraft_project::LayerId, Uid, SwatchId)> = Vec::new();
    for (cid, item) in &s.project.items {
        if only.is_some_and(|o| o != cid.0) {
            continue;
        }
        let effectcraft_project::ItemKind::Comp(comp) = &item.kind else { continue };
        for l in comp.layers.iter() {
            l.props.walk("", &mut |_, pr| {
                if pr.swatch.is_none()
                    && pr.keys.is_empty()
                    && !pr.has_expression()
                    && let Some(id) = matches(&pr.value)
                {
                    found.push((*cid, l.id, pr.uid, id));
                }
            });
        }
    }
    // Solids (not adjustment layers' solids: they draw no colour) that aren't linked yet.
    let adjustment: std::collections::HashSet<u64> = s
        .project
        .items
        .values()
        .filter_map(|it| match &it.kind {
            effectcraft_project::ItemKind::Comp(c) => Some(c),
            _ => None,
        })
        .flat_map(|c| c.layers.iter().filter(|l| l.switches.adjustment).filter_map(|l| l.source.item().map(|i| i.0)).collect::<Vec<_>>())
        .collect();
    let solids: Vec<(u64, SwatchId)> = if only.is_some() {
        vec![]
    } else {
        s.project
            .items
            .iter()
            .filter(|(id, _)| !s.project.swatches.solids.contains_key(&id.0) && !adjustment.contains(&id.0))
            .filter_map(|(id, it)| match &it.kind {
                effectcraft_project::ItemKind::Solid(sol) => {
                    matches(&KV::Color([f64::from(sol.color[0]), f64::from(sol.color[1]), f64::from(sol.color[2]), 1.0])).map(|sw| (id.0, sw))
                }
                _ => None,
            })
            .collect()
    };
    let mut per: std::collections::BTreeMap<SwatchId, usize> = Default::default();
    for f in &found {
        *per.entry(f.3).or_default() += 1;
    }
    for (_, sw) in &solids {
        *per.entry(*sw).or_default() += 1;
    }
    if !dry && (!found.is_empty() || !solids.is_empty()) {
        s.edit("Adopt Swatches", None, |proj, _| {
            for (cid, lid, uid, id) in &found {
                let l = super::layer_mut(proj, *cid, *lid)?;
                if let Some(pr) = l.props.find_mut(*uid) {
                    pr.swatch = Some(*id);
                }
            }
            for (item, sw) in &solids {
                proj.swatches.solids.insert(*item, *sw);
            }
            Ok(())
        })?;
    }
    let by_swatch: Vec<Value> =
        per.iter().map(|(id, n)| json!({"swatch": id, "name": s.project.swatches.get(*id).map(|x| x.name.clone()), "linked": n})).collect();
    Ok(json!({"linked": found.len() + solids.len(), "properties": found.len(), "solids": solids.len(), "dryRun": dry, "bySwatch": by_swatch}))
}

pub fn specs() -> Vec<CommandSpec> {
    vec![
        query!("swatch.list", "List Swatches", "{} → {theme, themes, swatches: [{id, name, group, color, hex, shown, shownHex, themes, uses}]}", list),
        cmd!("swatch.new", "New Swatch", [], None, "{name, color: [r,g,b(,a)] 0..1 | \"#rrggbb\", group?}", always, new),
        cmd!(
            "swatch.edit",
            "Edit Swatch",
            [],
            None,
            "{swatch: id|name, name?, color?, group?, theme?, themeColor? (the colour in `theme`; null removes it), merge?}",
            always,
            edit
        ),
        cmd!("swatch.delete", "Delete Swatch", [], None, "{swatch: id|name} (linked properties keep the colour they show)", always, delete),
        cmd!("swatch.link", "Link to Swatch", [], None, "{layer, path|prop (a path or a property uid), swatch: id|name, comp?}", has_comp, link),
        cmd!("swatch.unlink", "Unlink Swatch", [], None, "{layer, path|prop (a path or a property uid), comp?} (keeps the colour it showed)", has_comp, unlink),
        cmd!("swatch.theme", "Swatch Theme", [], None, "{theme: name | null (each swatch's own colour)}", always, theme),
        cmd!(
            "swatch.adopt",
            "Adopt Swatches",
            [],
            None,
            "{tolerance? (per channel, 0..1; default half an 8-bit level), comp?, dryRun?} → {linked, bySwatch} (links colour properties whose own colour matches a swatch)",
            always,
            adopt
        ),
        cmd!("swatch.linkSolid", "Link Solid to Swatch", [], None, "{item: a solid's id or name, swatch: id|name}", always, link_solid),
        cmd!("swatch.unlinkSolid", "Unlink Solid", [], None, "{item: a solid's id or name} (keeps its colour)", always, unlink_solid),
        query!("swatch.usage", "Swatch Usage", "{swatch: id|name} → {uses: [{comp, layer, prop, name}]}", usage),
    ]
}
