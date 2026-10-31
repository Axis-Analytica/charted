//! Evaluations: real-agent requests on the sample, each scored by checks on the model it
//! leaves. They catch prompt and tool regressions and compare agents and models. They
//! cost money and need a signed-in agent, so they run only on demand (`bin/eval`):
//!
//!   CHARTED_EVAL=build,yoy CHARTED_EVAL_MODEL=haiku bin/eval
//!
//! CHARTED_EVAL picks cases (default: all), CHARTED_EVAL_AGENT the agent (claude), and
//! CHARTED_EVAL_MODEL its model (sonnet). A summary goes to target/evals.jsonl.

use charted_agent::host::{AgentRequest, Host};
use charted_agent::tools::{self, Turn};
use charted_agent::transcript::{Item, TurnStatus};
use charted_core::api::{App, AppState, Request};
use charted_model::{Model, Visual};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

type Check = fn(&Model) -> Result<(), String>;

struct Case {
    name: &'static str,
    /// Changes made before the request (through the tools, as a script would).
    setup: &'static [(&'static str, &'static str)],
    request: &'static str,
    check: Check,
}

const CASES: &[Case] = &[
    Case {
        name: "build",
        setup: &[],
        request: "Build a page called Overview with total sales as a card, sales by month as a line chart and sales by region as a bar chart.",
        check: |m| {
            let p = page(m, "Overview")?;
            for ty in ["card", "lineChart"] {
                if !p.iter().any(|v| v.visual_type == ty) {
                    return Err(format!("no {ty}"));
                }
            }
            if !p
                .iter()
                .any(|v| v.visual_type.contains("arChart") || v.visual_type.contains("olumnChart"))
            {
                return Err("no bar chart".into());
            }
            no_overlaps(&p)
        },
    },
    Case {
        name: "yoy",
        setup: &[],
        request: "Add a measure for year-on-year sales growth as a percentage, and show it as a card on a new page called Growth.",
        check: |m| {
            let yoy = m
                .tables
                .iter()
                .flat_map(|t| &t.measures)
                .find(|x| {
                    let e = x.expression.to_uppercase();
                    e.contains("SAMEPERIODLASTYEAR")
                        || e.contains("DATEADD")
                        || e.contains("PARALLELPERIOD")
                        || e.contains("PREVIOUSYEAR")
                })
                .ok_or("no measure compares with the year before")?;
            let p = page(m, "Growth")?;
            let shown = p.iter().any(|v| {
                v.visual_type == "card"
                    && serde_json::to_string(&v.roles)
                        .unwrap_or_default()
                        .contains(&yoy.id)
            });
            shown
                .then_some(())
                .ok_or_else(|| format!("no card on Growth shows [{}]", yoy.name))
        },
    },
    Case {
        name: "tidy",
        setup: &[
            ("page", r#"{"action":"add","name":"Messy"}"#),
            (
                "add_visual",
                r#"{"page":"Messy","type":"card","title":"total sales","fields":{"values":["[Total Sales]"]},"position":{"x":40,"y":40,"width":300,"height":200}}"#,
            ),
            (
                "add_visual",
                r#"{"page":"Messy","type":"columnChart","title":"SALES BY REGION","fields":{"category":["'Store'[Region]"],"values":["[Total Sales]"]},"position":{"x":100,"y":100,"width":500,"height":300}}"#,
            ),
            (
                "add_visual",
                r#"{"page":"Messy","type":"donutChart","title":"by category","fields":{"legend":["'Product'[Category]"],"values":["[Total Sales]"]},"position":{"x":150,"y":150,"width":400,"height":300}}"#,
            ),
            (
                "add_visual",
                r#"{"page":"Messy","type":"slicer","fields":{"field":["'Store'[Region]"]},"position":{"x":500,"y":50,"width":300,"height":200}}"#,
            ),
        ],
        request: "Tidy up the page Messy: nothing overlapping, a sensible layout, and consistent titles.",
        check: |m| {
            let p = page(m, "Messy")?;
            if p.len() != 4 {
                return Err(format!("{} visuals, not 4", p.len()));
            }
            no_overlaps(&p)
        },
    },
    Case {
        name: "calcgroup",
        setup: &[],
        request: "Create a calculation group called Time Intelligence with items Current, YTD and Prior Year.",
        check: |m| {
            let g = m
                .tables
                .iter()
                .find_map(|t| t.calculation_group.as_ref().map(|g| (t, g)))
                .ok_or("no calculation group")?;
            let names: Vec<String> = g.1.items.iter().map(|i| i.name.to_lowercase()).collect();
            if names.len() < 3 {
                return Err(format!("items {names:?}"));
            }
            let ytd =
                g.1.items
                    .iter()
                    .any(|i| i.expression.to_uppercase().contains("YTD"));
            ytd.then_some(())
                .ok_or_else(|| "no item computes year to date".into())
        },
    },
    Case {
        name: "format",
        setup: &[
            ("page", r#"{"action":"add","name":"Formatting"}"#),
            (
                "add_visual",
                r#"{"page":"Formatting","type":"card","title":"Total Sales Amount","fields":{"values":["[Total Sales]"]}}"#,
            ),
            (
                "add_visual",
                r#"{"page":"Formatting","type":"columnChart","title":"Sales By Region","fields":{"category":["'Store'[Region]"],"values":["[Total Sales]"]}}"#,
            ),
        ],
        request: "On the page Formatting, make every visual title sentence case at 14 px.",
        check: |m| {
            for v in page(m, "Formatting")? {
                let t = &v.objects["title"];
                let text = t["text"].as_str().unwrap_or("");
                let rest: String = text.chars().skip(1).collect();
                if rest
                    .split(' ')
                    .any(|w| w.chars().next().is_some_and(char::is_uppercase))
                {
                    return Err(format!("title {text:?} isn't sentence case"));
                }
                if t["fontSize"].as_f64() != Some(14.0) {
                    return Err(format!("title {text:?} is {} px", t["fontSize"]));
                }
            }
            Ok(())
        },
    },
];

fn page<'a>(m: &'a Model, name: &str) -> Result<Vec<&'a Visual>, String> {
    m.report
        .pages
        .iter()
        .find(|p| p.settings.name.eq_ignore_ascii_case(name))
        .map(|p| p.visuals.iter().filter(|v| !v.hidden).collect())
        .ok_or_else(|| format!("no page {name}"))
}

fn no_overlaps(vs: &[&Visual]) -> Result<(), String> {
    for (i, a) in vs.iter().enumerate() {
        for b in &vs[i + 1..] {
            let (p, q) = (&a.position, &b.position);
            if p.x + p.width > q.x + 1.0
                && q.x + q.width > p.x + 1.0
                && p.y + p.height > q.y + 1.0
                && q.y + q.height > p.y + 1.0
            {
                return Err(format!("{} and {} overlap", a.visual_type, b.visual_type));
            }
        }
    }
    Ok(())
}

/// Runs the set-up steps as a script would (no one to ask, all allowed).
struct Script;
impl Turn for Script {
    fn group(&self) -> &str {
        "eval-setup"
    }
    fn label(&self) -> &str {
        "Set up"
    }
    fn data(&self) -> bool {
        true
    }
    fn confirm(&self, _: &str) -> bool {
        true
    }
    fn changed(&self, _: &AppState) {}
    fn draft(&self) -> Option<Vec<charted_compiler::steps::Query>> {
        None
    }
    fn set_draft(&self, _: Option<Vec<charted_compiler::steps::Query>>) {}
}

#[test]
#[ignore]
fn evals() {
    let pick = std::env::var("CHARTED_EVAL").unwrap_or_default();
    let agent = std::env::var("CHARTED_EVAL_AGENT").unwrap_or_else(|_| "claude".into());
    let model = std::env::var("CHARTED_EVAL_MODEL").unwrap_or_else(|_| "sonnet".into());
    let cases: Vec<&Case> = CASES
        .iter()
        .filter(|c| pick.is_empty() || pick.split(',').any(|p| p.trim() == c.name))
        .collect();
    assert!(
        !cases.is_empty(),
        "no case matches CHARTED_EVAL={pick}; cases: {}",
        CASES.iter().map(|c| c.name).collect::<Vec<_>>().join(", ")
    );
    let dir = tempfile::tempdir().unwrap();
    // SAFETY: set before any thread of this test reads the environment.
    unsafe {
        std::env::set_var("XDG_RUNTIME_DIR", dir.path().join("run"));
        std::env::set_var("CHARTED_AGENT_MCP", env!("CARGO_BIN_EXE_charted-headless"));
        std::env::remove_var("CHARTED_AGENT_CMD");
    }
    let mut results = vec![];
    for (n, case) in cases.iter().enumerate() {
        let app = Arc::new(Mutex::new(App::new(dir.path().join(format!("state{n}")))));
        app.lock()
            .unwrap()
            .handle(Request::OpenSample {
                dir: Some(dir.path().join(format!("p{n}")).display().to_string()),
            })
            .unwrap();
        for (tool, args) in case.setup {
            let args: Value = serde_json::from_str(args).unwrap();
            let o = tools::call(&app, &Script, tool, &args);
            assert!(!o.is_error, "set-up {tool}: {}", o.text);
        }
        let host = Host::start(app.clone(), |_| {}).unwrap();
        host.handle(AgentRequest::SetAgent {
            agent: agent.clone(),
            model: model.clone(),
        })
        .unwrap();
        host.handle(AgentRequest::Consent).unwrap();
        host.handle(AgentRequest::Send {
            text: case.request.into(),
            page: None,
            visuals: vec![],
        })
        .unwrap();
        let started = Instant::now();
        while host.status().running {
            if started.elapsed() > Duration::from_secs(600) {
                host.stop();
            }
            std::thread::sleep(Duration::from_millis(500));
        }
        let turn = host.status().turns.last().unwrap().clone();
        host.shutdown();
        let steps = turn
            .items
            .iter()
            .filter(|i| matches!(i, Item::Step { .. }))
            .count();
        let failed = turn
            .items
            .iter()
            .filter(|i| matches!(i, Item::Step { step } if !step.ok))
            .count();
        let m = app.lock().unwrap().session().unwrap().project.model.clone();
        let verdict = match turn.status {
            TurnStatus::Done => (case.check)(&m),
            s => Err(format!("the request ended {s:?}")),
        };
        let secs = started.elapsed().as_secs_f64();
        println!(
            "{:<10} {}  {:>4.0}s  {:>6}  {steps} steps ({failed} failed){}",
            case.name,
            if verdict.is_ok() { "PASS" } else { "FAIL" },
            secs,
            turn.cost_usd
                .map(|c| format!("${c:.3}"))
                .unwrap_or_else(|| "-".into()),
            verdict
                .as_ref()
                .err()
                .map(|e| format!("  — {e}"))
                .unwrap_or_default()
        );
        results.push(json!({
            "case": case.name, "agent": agent, "model": model, "pass": verdict.is_ok(),
            "error": verdict.err(), "seconds": secs.round(), "cost": turn.cost_usd,
            "steps": steps, "failedSteps": failed,
        }));
    }
    let passed = results.iter().filter(|r| r["pass"] == true).count();
    let cost: f64 = results.iter().filter_map(|r| r["cost"].as_f64()).sum();
    println!(
        "{passed}/{} passed, ${cost:.2} with {agent} {model}",
        results.len()
    );
    let out = concat!(env!("CARGO_MANIFEST_DIR"), "/../../target/evals.jsonl");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(out)
    {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let _ = writeln!(f, "{}", json!({ "at": at, "results": results }));
    }
}
