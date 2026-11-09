//! The sidebar: it collapses to a rail and back, dragging its edge past the snap point collapses
//! it, and in a narrow window it is a rail whose list opens over the page.

use crate::common::{SLOW, project, sandbox, start};
use nsh_core::prelude::{Point, Points, Size};

const COLLAPSE: &str = "label:Collapse the sidebar";
const EXPAND: &str = "label:Show the project list";
const CLOSE: &str = "label:Close the project list";

#[test]
fn the_sidebar_collapses_resizes_and_becomes_a_drawer_when_narrow() {
    let Some(dir) = sandbox("sidebar") else { return };
    project(&dir, "demo-repo");

    let mut app = start();
    app.wait_for_within("text:demo-repo", SLOW).unwrap();

    // Its button collapses it to the rail, where projects are initials, and opens it again.
    app.press(COLLAPSE).unwrap();
    app.wait_for_within(EXPAND, SLOW).unwrap();
    app.wait_for_within("text:DR", SLOW).unwrap();
    app.press(EXPAND).unwrap();
    app.wait_for_within(COLLAPSE, SLOW).unwrap();

    // Dragging its edge far to the left collapses it too.
    app.drag("label:Sidebar width", Point::new(Points(-200.0), Points(0.0))).unwrap();
    app.wait_for_within(EXPAND, SLOW).unwrap();
    app.press(EXPAND).unwrap();
    app.wait_for_within(COLLAPSE, SLOW).unwrap();

    // A narrow window keeps the rail; its button opens the list over the page, and choosing a
    // project closes it.
    app.set_viewport(Size::new(Points(600.0), Points(800.0))).unwrap();
    app.wait_for_within(EXPAND, SLOW).unwrap();
    app.press(EXPAND).unwrap();
    app.wait_for_within(CLOSE, SLOW).unwrap();
    app.press("text:demo-repo").unwrap();
    app.wait_for_gone(CLOSE, SLOW).unwrap();
    app.wait_for_within(EXPAND, SLOW).unwrap();

    let _ = std::fs::remove_dir_all(&dir);
}
