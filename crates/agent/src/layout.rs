//! Where the agent's visuals go: the first free spot that fits, on an 8 px grid with
//! 16 px margins and gaps, and whole-page arrangements.

use charted_model::Position;

pub const MARGIN: f64 = 16.0;
pub const GAP: f64 = 16.0;
const GRID: f64 = 8.0;

fn overlaps(a: &Position, b: &Position) -> bool {
    a.x < b.x + b.width + GAP
        && b.x < a.x + a.width + GAP
        && a.y < b.y + b.height + GAP
        && b.y < a.y + a.height + GAP
}

/// A spot for a `width` × `height` visual among `existing` on a page of `page_w` ×
/// `page_h`: the first free one scanning rows from the top left, else under everything
/// (the page then needs to grow: the second value is the height it needs).
pub fn place(
    page_w: f64,
    page_h: f64,
    existing: &[Position],
    width: f64,
    height: f64,
) -> (Position, f64) {
    let width = width.min(page_w - 2.0 * MARGIN).max(GRID);
    let z = existing.iter().map(|p| p.z).max().unwrap_or(0) + 1;
    let mut y = MARGIN;
    while y + height + MARGIN <= page_h {
        let mut x = MARGIN;
        while x + width + MARGIN <= page_w {
            let at = Position {
                x,
                y,
                width,
                height,
                z,
            };
            match existing
                .iter()
                .filter(|p| overlaps(&at, p))
                .map(|p| p.x + p.width + GAP)
                .reduce(f64::max)
            {
                None => return (at, page_h),
                // Skip past what's in the way.
                Some(right) => x = snap_up(right.max(x + GRID)),
            }
        }
        y += GRID;
    }
    let bottom = existing.iter().map(|p| p.y + p.height).fold(0.0, f64::max);
    let y = snap_up(if existing.is_empty() {
        MARGIN
    } else {
        bottom + GAP
    });
    (
        Position {
            x: MARGIN,
            y,
            width,
            height,
            z,
        },
        page_h.max(y + height + MARGIN),
    )
}

fn snap_up(n: f64) -> f64 {
    (n / GRID).ceil() * GRID
}

fn snap(n: f64) -> f64 {
    (n / GRID).floor() * GRID
}

/// What a visual is, for arranging.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Band {
    Slicer,
    Card,
    Chart,
    Table,
}

pub fn band(visual_type: &str) -> Band {
    match visual_type {
        "slicer" => Band::Slicer,
        "card" | "multiRowCard" | "kpi" | "gauge" | "textbox" => Band::Card,
        "table" | "matrix" => Band::Table,
        _ => Band::Chart,
    }
}

/// Positions for every visual (in the order given, by band): slicers across the top,
/// then cards in a row, then charts two (or three) to a row, then tables full width.
/// Returns the positions and the page height needed (never less than `page_h`).
pub fn arrange(
    page_w: f64,
    page_h: f64,
    visuals: &[(String, &str)],
) -> (Vec<(String, Position)>, f64) {
    let inner = page_w - 2.0 * MARGIN;
    let mut ordered: Vec<(usize, &(String, &str))> = visuals.iter().enumerate().collect();
    ordered.sort_by_key(|(i, (_, t))| (band(t), *i));
    let count = |b: Band| visuals.iter().filter(|(_, t)| band(t) == b).count();
    let charts = count(Band::Chart);
    // Height left for charts once the fixed bands are placed.
    let fixed = [
        (Band::Slicer, 72.0),
        (Band::Card, 120.0),
        (Band::Table, 280.0),
    ]
    .iter()
    .filter(|(b, _)| count(*b) > 0)
    .map(|(_, h)| h + GAP)
    .sum::<f64>();
    let per_row = if charts >= 5 { 3 } else { 2 };
    let rows = charts.div_ceil(per_row).max(1) as f64;
    let chart_h = snap(((page_h - 2.0 * MARGIN - fixed - (rows - 1.0) * GAP) / rows).max(240.0));
    let mut out = Vec::new();
    let mut y = MARGIN;
    let mut z = 0;
    for b in [Band::Slicer, Band::Card, Band::Chart, Band::Table] {
        let items: Vec<&String> = ordered
            .iter()
            .filter(|(_, (_, t))| band(t) == b)
            .map(|(_, (id, _))| id)
            .collect();
        if items.is_empty() {
            continue;
        }
        let (cols, h) = match b {
            Band::Slicer => (items.len().min(5), 72.0),
            Band::Card => (items.len().min(6), 120.0),
            Band::Chart => (per_row.min(items.len()), chart_h),
            Band::Table => (1, 280.0),
        };
        for row in items.chunks(cols) {
            let n = row.len() as f64;
            // A short last row of charts keeps the others' width.
            let across = if b == Band::Chart { cols as f64 } else { n };
            let w = snap((inner - (across - 1.0) * GAP) / across);
            for (i, id) in row.iter().enumerate() {
                z += 1;
                out.push((
                    (*id).clone(),
                    Position {
                        x: MARGIN + i as f64 * (w + GAP),
                        y,
                        width: w,
                        height: h,
                        z,
                    },
                ));
            }
            y += h + GAP;
        }
    }
    let needed = y - GAP + MARGIN;
    (out, page_h.max(needed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn clear(a: &Position, b: &Position) -> bool {
        a.x + a.width <= b.x
            || b.x + b.width <= a.x
            || a.y + a.height <= b.y
            || b.y + b.height <= a.y
    }

    #[test]
    fn the_first_visual_goes_top_left_and_the_next_beside_it() {
        let (a, h) = place(1280.0, 720.0, &[], 400.0, 260.0);
        assert_eq!((a.x, a.y, h), (16.0, 16.0, 720.0));
        let (b, _) = place(1280.0, 720.0, &[a], 400.0, 260.0);
        assert_eq!((b.x, b.y), (432.0, 16.0));
    }

    #[test]
    fn a_full_page_grows() {
        let big = Position {
            x: 0.0,
            y: 0.0,
            width: 1280.0,
            height: 700.0,
            z: 1,
        };
        let (p, h) = place(1280.0, 720.0, &[big], 400.0, 260.0);
        assert_eq!(p.y, 720.0);
        assert!(h >= p.y + p.height);
    }

    #[test]
    fn arrange_puts_slicers_first_and_tables_last() {
        let v = [
            ("t", "table"),
            ("c", "lineChart"),
            ("s", "slicer"),
            ("k", "card"),
        ]
        .map(|(a, b)| (a.to_string(), b));
        let (out, _) = arrange(1280.0, 720.0, &v);
        let y = |id: &str| out.iter().find(|(i, _)| i == id).unwrap().1.y;
        assert!(y("s") < y("k") && y("k") < y("c") && y("c") < y("t"));
    }

    proptest! {
        #[test]
        fn placed_visuals_never_overlap(sizes in prop::collection::vec((80u32..700, 60u32..500), 1..14)) {
            let mut placed: Vec<Position> = Vec::new();
            let mut h = 720.0;
            for (w, ht) in sizes {
                let (p, need) = place(1280.0, h, &placed, w as f64, ht as f64);
                h = need;
                prop_assert!(p.x >= 0.0 && p.x + p.width <= 1280.0 && p.y + p.height <= h);
                for q in &placed {
                    prop_assert!(clear(&p, q));
                }
                placed.push(p);
            }
        }

        #[test]
        fn arranged_visuals_never_overlap(types in prop::collection::vec(0usize..4, 1..16)) {
            let names = ["slicer", "card", "lineChart", "matrix"];
            let v: Vec<(String, &str)> = types.iter().enumerate().map(|(i, t)| (i.to_string(), names[*t])).collect();
            let (out, h) = arrange(1280.0, 720.0, &v);
            prop_assert_eq!(out.len(), v.len());
            for (i, (_, a)) in out.iter().enumerate() {
                prop_assert!(a.x + a.width <= 1280.0 && a.y + a.height <= h);
                for (_, b) in &out[i + 1..] {
                    prop_assert!(clear(a, b));
                }
            }
        }
    }
}
