//! Project swatches (#3362): a colour property linked to a swatch shows the swatch's colour in the
//! active theme; editing the swatch, switching the theme, unlinking and deleting behave as the
//! commands say, through renders, expressions, undo and a save.

use effectcraft_project::{ItemId, LayerId, Project};
use effectcraft_time::Tick;
use serde_json::{Value, json};

use crate::Session;
use crate::render::RenderOpts;

const FILL: &str = "effects/ec.generate.fill/color";

/// A 16x16 comp with a white solid under a Fill effect (red by default).
fn setup() -> (Session, ItemId, u64) {
    let mut s = Session::default();
    let comp = s.execute_checked("comp.new", json!({"width": 16, "height": 16, "frameRate": 25, "duration": 2})).unwrap()["comp"].as_u64().unwrap();
    let l = s.execute_checked("layer.newSolid", json!({"color": "#ffffff"})).unwrap()["layer"].as_u64().unwrap();
    s.execute_checked("effect.apply", json!({"layers": [l], "effect": "ec.generate.fill"})).unwrap();
    (s, ItemId(comp), l)
}

fn px(s: &Session, comp: ItemId) -> [f32; 4] {
    s.render(comp, Tick::from_seconds_f64(0.5), RenderOpts::default()).get(8, 8)
}

fn close(p: [f32; 4], want: [f32; 3]) -> bool {
    (0..3).all(|i| (p[i] - want[i]).abs() < 0.01)
}

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    s.execute_checked(id, p).unwrap_or_else(|e| panic!("{id}: {e}"))
}

#[test]
fn a_linked_colour_follows_its_swatch_and_the_theme() {
    let (mut s, comp, l) = setup();
    assert!(close(px(&s, comp), [1.0, 0.0, 0.0]), "Fill's own red");
    let navy = run(&mut s, "swatch.new", json!({"name": "Navy", "color": "#203060", "group": "Brand"}))["swatch"].as_u64().unwrap();
    run(&mut s, "swatch.link", json!({"layer": l, "path": FILL, "swatch": "Navy"}));
    let navy_rgb = [32.0 / 255.0, 48.0 / 255.0, 96.0 / 255.0];
    assert!(close(px(&s, comp), navy_rgb), "{:?}", px(&s, comp));

    // Editing the swatch recolours the link.
    run(&mut s, "swatch.edit", json!({"swatch": navy, "color": [0.0, 0.0, 1.0]}));
    assert!(close(px(&s, comp), [0.0, 0.0, 1.0]));

    // A theme picks the swatch's colour there; a swatch without one keeps its own.
    run(&mut s, "swatch.edit", json!({"swatch": "Navy", "theme": "Sponsor", "themeColor": "#20b0a0"}));
    run(&mut s, "swatch.theme", json!({"theme": "Sponsor"}));
    assert!(close(px(&s, comp), [32.0 / 255.0, 176.0 / 255.0, 160.0 / 255.0]));
    run(&mut s, "swatch.theme", json!({"theme": "Other"}));
    assert!(close(px(&s, comp), [0.0, 0.0, 1.0]));
    run(&mut s, "swatch.theme", json!({"theme": null}));
    let list = run(&mut s, "swatch.list", json!({}));
    assert_eq!(list["themes"], json!(["Sponsor"]));
    assert_eq!(list["swatches"][0]["uses"], json!(1));
}

#[test]
fn keyframes_wait_while_linked_and_unlink_keeps_the_colour() {
    let (mut s, comp, l) = setup();
    run(&mut s, "prop.addKey", json!({"layer": l, "path": FILL, "time": 0, "value": [0.0, 1.0, 0.0, 1.0]}));
    run(&mut s, "prop.addKey", json!({"layer": l, "path": FILL, "time": 1, "value": [0.0, 1.0, 0.0, 1.0]}));
    assert!(close(px(&s, comp), [0.0, 1.0, 0.0]), "keyed green");
    run(&mut s, "swatch.new", json!({"name": "Blue", "color": "#2050c0"}));
    run(&mut s, "swatch.link", json!({"layer": l, "path": FILL, "swatch": "Blue"}));
    assert!(close(px(&s, comp), [32.0 / 255.0, 80.0 / 255.0, 192.0 / 255.0]), "linked: the swatch, not the keys");
    run(&mut s, "swatch.unlink", json!({"layer": l, "path": FILL}));
    assert!(close(px(&s, comp), [0.0, 1.0, 0.0]), "unlinked: the keys apply again");
}

#[test]
fn deleting_a_swatch_leaves_the_colour_it_showed() {
    let (mut s, comp, l) = setup();
    run(&mut s, "swatch.new", json!({"name": "Teal", "color": "#20b0a0"}));
    run(&mut s, "swatch.link", json!({"layer": l, "path": FILL, "swatch": "Teal"}));
    let r = run(&mut s, "swatch.delete", json!({"swatch": "Teal"}));
    assert_eq!(r["unlinked"], json!(1));
    assert!(close(px(&s, comp), [32.0 / 255.0, 176.0 / 255.0, 160.0 / 255.0]));
    let lay = s.active_comp().unwrap().layer(LayerId(l)).unwrap();
    assert_eq!(lay.props.prop(&crate::cmd::resolve_effect_path(lay, FILL)).unwrap().swatch, None);
    // Undo brings the swatch and the link back.
    run(&mut s, "edit.undo", json!({}));
    assert_eq!(run(&mut s, "swatch.usage", json!({"swatch": "Teal"}))["uses"].as_array().unwrap().len(), 1);
}

#[test]
fn links_and_swatches_survive_a_save_and_bad_input_is_refused() {
    let (mut s, _, l) = setup();
    run(&mut s, "swatch.new", json!({"name": "Navy", "color": "#203060"}));
    run(&mut s, "swatch.link", json!({"layer": l, "path": FILL, "swatch": "Navy"}));
    let json = serde_json::to_string(&s.project).unwrap();
    let back: Project = serde_json::from_str(&json).unwrap();
    assert_eq!(back.swatches, s.project.swatches);
    // A project without swatches writes no `swatches` field.
    assert!(!serde_json::to_string(&Project::default()).unwrap().contains("swatches"));
    for (id, p) in [
        ("swatch.new", json!({"name": "Navy", "color": "#000000"})),
        ("swatch.new", json!({"name": "X", "color": "#zz0000"})),
        ("swatch.new", json!({"name": "X", "color": [1.0, f64::MAX]})),
        ("swatch.link", json!({"layer": l, "path": "effects/ec.generate.fill/opacity", "swatch": "Navy"})),
        ("swatch.link", json!({"layer": l, "path": FILL, "swatch": "Nope"})),
        ("swatch.edit", json!({"swatch": "Navy", "themeColor": "#ffffff"})),
    ] {
        assert!(s.execute_checked(id, p.clone()).is_err(), "{id} {p}");
    }
}

#[test]
fn adopt_links_the_colours_that_match_a_swatch() {
    let (mut s, comp, l) = setup();
    run(&mut s, "prop.set", json!({"layer": l, "path": FILL, "value": [32.0 / 255.0, 48.0 / 255.0, 96.0 / 255.0, 1.0]}));
    run(&mut s, "swatch.new", json!({"name": "Navy", "color": "#203060"}));
    run(&mut s, "swatch.new", json!({"name": "Teal", "color": "#20b0a0"}));
    let dry = run(&mut s, "swatch.adopt", json!({"dryRun": true}));
    assert_eq!(dry["linked"], json!(1));
    assert_eq!(run(&mut s, "swatch.usage", json!({"swatch": "Navy"}))["uses"].as_array().unwrap().len(), 0, "a dry run links nothing");
    let r = run(&mut s, "swatch.adopt", json!({}));
    assert_eq!(r["bySwatch"][0]["name"], json!("Navy"));
    // Adopted: a brand change now recolours it.
    run(&mut s, "swatch.edit", json!({"swatch": "Navy", "color": "#20b0a0"}));
    assert!(close(px(&s, comp), [32.0 / 255.0, 176.0 / 255.0, 160.0 / 255.0]));
    assert!(s.execute_checked("swatch.adopt", json!({"tolerance": 2})).is_err());
}

#[test]
fn a_linked_solid_follows_its_swatch_and_adopt_finds_solids() {
    let mut s = Session::default();
    let comp = ItemId(run(&mut s, "comp.new", json!({"width": 16, "height": 16, "frameRate": 25, "duration": 2}))["comp"].as_u64().unwrap());
    let l = run(&mut s, "layer.newSolid", json!({"color": "#2050c0", "name": "Cell"}))["layer"].as_u64().unwrap();
    let item = s.active_comp().unwrap().layer(LayerId(l)).unwrap().source.item().unwrap().0;
    run(&mut s, "swatch.new", json!({"name": "Blue", "color": "#2050c0"}));
    let r = run(&mut s, "swatch.adopt", json!({}));
    assert_eq!((r["solids"].clone(), r["properties"].clone()), (json!(1), json!(0)));
    run(&mut s, "swatch.edit", json!({"swatch": "Blue", "theme": "Alternate", "themeColor": "#c0a040"}));
    run(&mut s, "swatch.theme", json!({"theme": "Alternate"}));
    assert!(close(px(&s, comp), [192.0 / 255.0, 160.0 / 255.0, 64.0 / 255.0]), "{:?}", px(&s, comp));
    assert_eq!(run(&mut s, "swatch.usage", json!({"swatch": "Blue"}))["solids"][0]["item"], json!(item));
    run(&mut s, "swatch.unlinkSolid", json!({"item": item}));
    run(&mut s, "swatch.theme", json!({"theme": null}));
    assert!(close(px(&s, comp), [192.0 / 255.0, 160.0 / 255.0, 64.0 / 255.0]), "unlinked: keeps its colour");
    run(&mut s, "swatch.linkSolid", json!({"item": item, "swatch": "Blue"}));
    assert!(close(px(&s, comp), [32.0 / 255.0, 80.0 / 255.0, 192.0 / 255.0]));
    // Deleting the swatch drops the link; the solid keeps its colour.
    run(&mut s, "swatch.delete", json!({"swatch": "Blue"}));
    assert!(s.project.swatches.solids.is_empty());
    assert!(close(px(&s, comp), [32.0 / 255.0, 80.0 / 255.0, 192.0 / 255.0]));
}

#[test]
fn analysis_keys_change_with_a_linked_swatch_only() {
    // A Warp Stabilizer / Camera Tracker / Roto Brush analysis keyed on the layer's frames must
    // be redone when a swatch it shows changes, and kept when another swatch changes.
    let (mut s, comp, l) = setup();
    run(&mut s, "swatch.new", json!({"name": "Linked", "color": "#203060"}));
    run(&mut s, "swatch.new", json!({"name": "Other", "color": "#20b0a0"}));
    run(&mut s, "swatch.link", json!({"layer": l, "path": FILL, "swatch": "Linked"}));
    let key = |s: &Session| {
        let layer = s.project.comp(comp).unwrap().layer(LayerId(l)).unwrap();
        crate::analysis::frames_key::signature(&s.project, comp, layer)
    };
    let before = key(&s);
    run(&mut s, "swatch.edit", json!({"swatch": "Other", "color": "#ffffff"}));
    assert_eq!(key(&s), before, "an unlinked swatch changes no frames");
    run(&mut s, "swatch.edit", json!({"swatch": "Linked", "color": "#ffffff"}));
    assert_ne!(key(&s), before, "the linked swatch changes the frames");
}
