//! `swatch(name, theme?)`: a project swatch's colour by name, in the active theme or the one named
//! (#3362).

use super::*;
use effectcraft_project::swatches::Swatch;

fn with_swatches() -> Fx {
    let mut f = fx();
    let themes = [("Light".to_string(), [1.0, 1.0, 1.0, 1.0])].into_iter().collect();
    f.p.swatches.list.push(Swatch { id: 900, name: "Navy".into(), group: "Brand".into(), color: [0.0, 0.25, 0.5, 1.0], themes });
    f
}

#[test]
fn swatch_reads_the_colour_in_the_active_or_named_theme() {
    let mut f = with_swatches();
    assert_close!(f.num("swatch('Navy')[2]", 0.0), 0.5);
    assert_close!(f.num("swatch('navy')[1]", 0.0), 0.25);
    assert_close!(f.num("swatch('Navy', 'Light')[2]", 0.0), 1.0);
    // A theme the swatch has no colour for: its own colour.
    assert_close!(f.num("swatch('Navy', 'Dark')[2]", 0.0), 0.5);
    f.p.swatches.theme = Some("Light".into());
    assert_close!(f.num("swatch('Navy')[2]", 0.0), 1.0);
    assert_close!(f.num("try { swatch('Nope'); 1 } catch (e) { -1 }", 0.0), -1.0);
}
