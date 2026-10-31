//! Reshaping charted itself (the user's layout, crates/core/src/workspace.rs): panes docked,
//! floating or hidden, a second view beside the first, buttons hidden, and layout profiles.
//! A request's layout changes undo as one step (Layout menu → Undo layout change).

use charted_core::workspace::{FloatRect, Layout, PANES, Place, Split, ViewKind, default_place};
use serde_json::{Value, json};
use std::fmt::Write;

use super::{Cx, Done, Fail, Res, Step, ToolDef, action, fail, opt_str, schema, str_arg, str_list};

pub fn defs() -> Vec<ToolDef> {
    vec![
        ToolDef {
            name: "describe_workspace",
            description: "How charted itself is laid out for the user: the layout profiles and which is active, where each pane is (left, right, float, hidden), the second view beside the first, and the hidden buttons.".into(),
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "set_pane",
            description: format!(
                "Moves a report pane: place left, right, float or hidden (left and right are the reading start and end, mirrored in right-to-left languages); width for a docked pane (180–640 px); x, y, width, height for a floating one. Panes: {}.",
                PANES.join(", ")
            ),
            input_schema: schema(
                json!({
                    "pane": { "type": "string", "enum": PANES },
                    "place": { "type": "string", "enum": ["left", "right", "float", "hidden"] },
                    "width": { "type": "number" }, "height": { "type": "number" }, "x": { "type": "number" }, "y": { "type": "number" }
                }),
                &["pane", "place"],
            ),
        },
        ToolDef {
            name: "split_view",
            description: "Shows a second view beside the one picked in the rail (report, model, data or query; a second report shows its own page), or none for one view. ratio is the first view's share of the width (0.2–0.8).".into(),
            input_schema: schema(
                json!({ "view": { "type": "string", "enum": ["none", "report", "model", "data", "query"] }, "ratio": { "type": "number" } }),
                &["view"],
            ),
        },
        ToolDef {
            name: "list_commands",
            description: "The buttons the user can hide, with where each shows and how often the user has used it on this machine (to suggest hiding the ones never used).".into(),
            input_schema: schema(json!({}), &[]),
        },
        ToolDef {
            name: "hide_commands",
            description: "Hides buttons (by id, from list_commands) from toolbars and menus; they stay in the command palette (Ctrl+K). The user is asked first. show: true brings them back.".into(),
            input_schema: schema(
                json!({ "ids": { "type": "array", "items": { "type": "string" } }, "show": { "type": "boolean" } }),
                &["ids"],
            ),
        },
        ToolDef {
            name: "remember",
            description: "Adds a line to the project's instructions (agent.md), which you get with every request: for rules the user wants kept (\"always use the Date table\", \"titles in sentence case\"). The user is asked first. Only when the user asks you to remember something, or agrees to it.".into(),
            input_schema: schema(json!({ "instruction": { "type": "string" } }), &["instruction"]),
        },
        ToolDef {
            name: "profile",
            description: "Layout profiles: create (a copy of the active one, made active), switch, rename (new_name), delete, or reset (a built-in back as it ships). Built-ins: Standard, Report + model, Two report pages.".into(),
            input_schema: schema(
                json!({ "action": { "type": "string", "enum": ["create", "switch", "rename", "delete", "reset"] }, "name": { "type": "string" }, "new_name": { "type": "string" } }),
                &["action", "name"],
            ),
        },
    ]
}

pub fn call(cx: &Cx, name: &str, args: &Value) -> Option<Res<Done>> {
    Some(match name {
        "describe_workspace" => describe(cx),
        "set_pane" => set_pane(cx, args),
        "split_view" => split_view(cx, args),
        "list_commands" => list_commands(cx),
        "remember" => remember(cx, args),
        "hide_commands" => hide_commands(cx, args),
        "profile" => profile(cx, args),
        _ => return None,
    })
}

fn place_name(p: Place) -> &'static str {
    match p {
        Place::Left => "left",
        Place::Right => "right",
        Place::Float => "float",
        Place::Hidden => "hidden",
    }
}

fn view_name(v: ViewKind) -> &'static str {
    match v {
        ViewKind::Report => "report",
        ViewKind::Data => "data",
        ViewKind::Model => "model",
        ViewKind::Query => "query",
    }
}

fn describe(cx: &Cx) -> Res<Done> {
    let app = cx.lock();
    let w = &app.layouts().workspace;
    let mut out = String::new();
    for p in &w.profiles {
        let _ = writeln!(
            out,
            "profile \"{}\"{}{}",
            p.name,
            if p.id == w.active { " (active)" } else { "" },
            if p.builtin.is_some() { " built-in" } else { "" }
        );
    }
    let l = &w.active().layout;
    for pane in PANES {
        let pl = l.panes.get(*pane);
        let place = pl.map_or(default_place(pane), |p| p.place);
        let _ = write!(out, "pane {pane}: {}", place_name(place));
        if let Some(width) = pl.and_then(|p| p.width) {
            let _ = write!(out, " width {width}");
        }
        out.push('\n');
    }
    let _ = writeln!(
        out,
        "second view: {}",
        l.split.map_or("none".to_string(), |s| format!(
            "{} (ratio {:.2})",
            view_name(s.view),
            s.ratio
        ))
    );
    let _ = writeln!(
        out,
        "hidden buttons: {}",
        if l.hidden.is_empty() {
            "none".into()
        } else {
            l.hidden.join(", ")
        }
    );
    Done::read(out)
}

/// Changes the active layout in the request's undo group, and shows it.
fn change(cx: &Cx, f: impl FnOnce(Layout) -> Res<Layout>, step: Step) -> Res<Done> {
    let state = {
        let mut app = cx.lock();
        let l = f(app.layouts().workspace.active().layout.clone())?;
        app.set_layout_grouped(l, cx.turn.group())
            .map_err(|e| Fail(e.message))?
    };
    cx.turn.changed(&state);
    Ok(Done {
        text: format!("{}.", step.text),
        step: Some(step),
    })
}

fn set_pane(cx: &Cx, args: &Value) -> Res<Done> {
    let pane = str_arg(args, "pane")?;
    if !PANES.contains(&pane) {
        return fail(format!(
            "there's no pane {pane}; panes: {}",
            PANES.join(", ")
        ));
    }
    let place = match str_arg(args, "place")? {
        "left" => Place::Left,
        "right" => Place::Right,
        "float" => Place::Float,
        "hidden" => Place::Hidden,
        p => return fail(format!("place {p} isn't left, right, float or hidden")),
    };
    let n = |k: &str| args.get(k).and_then(Value::as_f64);
    let op = match place {
        Place::Hidden => "Hid the %1 pane",
        Place::Float => "Floated the %1 pane",
        Place::Left => "Docked the %1 pane on the left",
        Place::Right => "Docked the %1 pane on the right",
    };
    change(
        cx,
        |mut l| {
            let mut p = l
                .panes
                .get(pane)
                .cloned()
                .unwrap_or(charted_core::workspace::PaneLayout {
                    place,
                    width: None,
                    float: None,
                });
            p.place = place;
            if place == Place::Float {
                let f = p.float.unwrap_or(FloatRect {
                    x: 24.0,
                    y: 24.0,
                    width: 300.0,
                    height: 460.0,
                });
                p.float = Some(FloatRect {
                    x: n("x").unwrap_or(f.x),
                    y: n("y").unwrap_or(f.y),
                    width: n("width").unwrap_or(f.width),
                    height: n("height").unwrap_or(f.height),
                });
            } else if let Some(w) = n("width") {
                p.width = Some(w);
            }
            l.panes.insert(pane.into(), p);
            Ok(l)
        },
        Step::new(op, &[pane]),
    )
}

fn split_view(cx: &Cx, args: &Value) -> Res<Done> {
    let view = match str_arg(args, "view")? {
        "none" => None,
        "report" => Some(ViewKind::Report),
        "model" => Some(ViewKind::Model),
        "data" => Some(ViewKind::Data),
        "query" => Some(ViewKind::Query),
        v => return fail(format!("view {v} isn't none, report, model, data or query")),
    };
    let step = match view {
        None => Step::new("Back to one view", &[]),
        Some(v) => Step::new("Put the %1 view beside the first", &[view_name(v)]),
    };
    change(
        cx,
        |mut l| {
            let ratio = args
                .get("ratio")
                .and_then(Value::as_f64)
                .or(l.split.map(|s| s.ratio))
                .unwrap_or(0.5);
            l.split = view.map(|view| Split { view, ratio });
            Ok(l)
        },
        step,
    )
}

fn list_commands(cx: &Cx) -> Res<Done> {
    let app = cx.lock();
    let hidden = &app.layouts().workspace.active().layout.hidden;
    let cmds = app.commands();
    if cmds.is_empty() {
        return Done::read(
            "The window hasn't listed its buttons yet; ask the user to open the report view.",
        );
    }
    let mut out = String::new();
    for c in cmds {
        let _ = writeln!(
            out,
            "{} \"{}\" in the {}: used {} times{}",
            c.id,
            c.label,
            c.place,
            c.uses,
            if hidden.contains(&c.id) {
                " (hidden)"
            } else {
                ""
            }
        );
    }
    Done::read(out)
}

fn hide_commands(cx: &Cx, args: &Value) -> Res<Done> {
    let ids = str_list(args, "ids");
    if ids.is_empty() {
        return fail("ids lists the buttons (from list_commands)");
    }
    let show = args.get("show").and_then(Value::as_bool).unwrap_or(false);
    let labels: Vec<String> = {
        let app = cx.lock();
        let known = app.commands();
        ids.iter()
            .map(|id| {
                if known.is_empty() {
                    Ok(id.clone())
                } else {
                    known
                        .iter()
                        .find(|c| &c.id == id)
                        .map(|c| c.label.clone())
                        .ok_or_else(|| Fail(format!("there's no button {id}; see list_commands")))
                }
            })
            .collect::<Res<_>>()?
    };
    let list = labels.join(", ");
    if !show && !cx.turn.confirm(&format!("Hide these buttons: {list}")) {
        return fail("the user didn't want those buttons hidden; leave them");
    }
    change(
        cx,
        |mut l| {
            if show {
                l.hidden.retain(|h| !ids.contains(h));
            } else {
                l.hidden.extend(ids.iter().cloned());
            }
            Ok(l)
        },
        Step::new(if show { "Showed %1" } else { "Hid %1" }, &[&list]),
    )
}

fn remember(cx: &Cx, args: &Value) -> Res<Done> {
    let line = str_arg(args, "instruction")?.trim();
    if line.is_empty() || line.contains('\n') {
        return fail("instruction is one line");
    }
    let dir = cx
        .lock()
        .session()
        .map(|s| s.project.dir.clone())
        .ok_or_else(|| Fail("no project is open".into()))?;
    if !cx
        .turn
        .confirm(&format!("Add to the project's instructions: \"{line}\""))
    {
        return fail("the user didn't want that remembered; carry on without it");
    }
    crate::settings::add_instruction(&dir, line).map_err(Fail)?;
    Ok(Done {
        text: "Added to agent.md.".into(),
        step: Some(Step::new("Remembered \"%1\"", &[line])),
    })
}

fn profile(cx: &Cx, args: &Value) -> Res<Done> {
    let act = action(args, &["create", "switch", "rename", "delete", "reset"])?;
    let name = str_arg(args, "name")?;
    if act == "delete"
        && !cx
            .turn
            .confirm(&format!("Delete the layout profile \"{name}\""))
    {
        return fail("the user didn't want that profile deleted; leave it");
    }
    let state = {
        let mut app = cx.lock();
        let l = app.layouts_mut();
        match act {
            "create" => l.add(name, None).map(|_| ()),
            "switch" => l.switch(name),
            "rename" => l.rename(
                name,
                opt_str(args, "new_name").ok_or_else(|| Fail("rename needs new_name".into()))?,
            ),
            "delete" => l.delete(name),
            _ => l.reset(name),
        }
        .map_err(Fail)?;
        app.save_workspace();
        app.state().map_err(|e| Fail(e.to_string()))?
    };
    cx.turn.changed(&state);
    let op = match act {
        "create" => "Made the layout profile \"%1\"",
        "switch" => "Switched to the layout \"%1\"",
        "rename" => "Renamed the layout \"%1\"",
        "delete" => "Deleted the layout \"%1\"",
        _ => "Reset the layout \"%1\"",
    };
    let step = Step::new(op, &[name]);
    Ok(Done {
        text: format!("{}.", step.text),
        step: Some(step),
    })
}
