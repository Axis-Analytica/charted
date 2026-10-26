//! Generates the `OSSIE_SQL_2026` expression written next to each measure's DAX, so
//! tools without DAX can evaluate it. Columns are `"Dataset"."Field"` (Ossie names),
//! referenced measures are expanded inline (Ossie metrics cannot refer to each other).
//! Measures that cannot be expressed this way get no generated expression.

use charted_dax::{AggFunc, BinaryOp, Bound, ScalarFunc, TableExpr, UnaryOp};
use charted_model::{Id, Model};

use crate::quote::{ident, string};

pub fn measure_sql(model: &Model, measure: &str) -> Option<String> {
    expand(model, measure, &mut Vec::new())
}

fn expand(model: &Model, id: &str, stack: &mut Vec<Id>) -> Option<String> {
    if stack.iter().any(|s| s == id) {
        return None;
    }
    let m = model
        .tables
        .iter()
        .flat_map(|t| &t.measures)
        .find(|m| m.id == id)?;
    let (bound, _) = charted_dax::check(model, &m.expression);
    stack.push(id.to_string());
    let out = sql(model, &bound?, stack);
    stack.pop();
    out
}

fn column(model: &Model, table: &str, column: &str) -> Option<String> {
    let t = model.table(table)?;
    let c = t.column(column)?;
    Some(format!("{}.{}", ident(&t.name), ident(&c.name)))
}

fn sql(model: &Model, b: &Bound, stack: &mut Vec<Id>) -> Option<String> {
    Some(match b {
        Bound::Number(n) => n.to_string(),
        Bound::Text(s) => string(s),
        Bound::Bool(v) => v.to_string().to_uppercase(),
        Bound::Blank => "NULL".into(),
        Bound::Column { table, column: c } => column(model, table, c)?,
        Bound::Measure { measure, .. } => format!("({})", expand(model, measure, stack)?),
        // Only aggregates over a whole table translate; filter context changes do not.
        Bound::Aggregate {
            func,
            arg,
            source: TableExpr::Table(_),
        } => {
            let x = match arg {
                Some(a) => sql(model, a, stack)?,
                None => "*".into(),
            };
            match func {
                AggFunc::Sum => format!("SUM({x})"),
                AggFunc::Average => format!("AVG({x})"),
                AggFunc::Min => format!("MIN({x})"),
                AggFunc::Max => format!("MAX({x})"),
                AggFunc::Count | AggFunc::CountRows => format!("COUNT({x})"),
                AggFunc::DistinctCount => format!("COUNT(DISTINCT {x})"),
                AggFunc::CountBlank => format!("SUM(CASE WHEN {x} IS NULL THEN 1 ELSE 0 END)"),
            }
        }
        Bound::Call { func, args } => {
            let a = args
                .iter()
                .map(|x| sql(model, x, stack))
                .collect::<Option<Vec<_>>>()?;
            match func {
                ScalarFunc::Divide => format!(
                    "CASE WHEN {b} = 0 OR {b} IS NULL THEN {alt} ELSE {x} / {b} END",
                    x = a[0],
                    b = a[1],
                    alt = a.get(2).map(String::as_str).unwrap_or("NULL")
                ),
                ScalarFunc::If => format!(
                    "CASE WHEN {} THEN {} ELSE {} END",
                    a[0],
                    a[1],
                    a.get(2).map(String::as_str).unwrap_or("NULL")
                ),
                ScalarFunc::Abs => format!("ABS({})", a[0]),
                ScalarFunc::Round => format!("ROUND({}, {})", a[0], a[1]),
                ScalarFunc::Coalesce => format!("COALESCE({})", a.join(", ")),
                ScalarFunc::And => format!("({} AND {})", a[0], a[1]),
                ScalarFunc::Or => format!("({} OR {})", a[0], a[1]),
                ScalarFunc::Not => format!("(NOT {})", a[0]),
                ScalarFunc::IsBlank => format!("({} IS NULL)", a[0]),
                ScalarFunc::Year => format!("EXTRACT(YEAR FROM {})", a[0]),
                ScalarFunc::Month => format!("EXTRACT(MONTH FROM {})", a[0]),
                ScalarFunc::Day => format!("EXTRACT(DAY FROM {})", a[0]),
                ScalarFunc::Quarter => format!("EXTRACT(QUARTER FROM {})", a[0]),
                ScalarFunc::Int => format!("FLOOR({})", a[0]),
                ScalarFunc::Upper => format!("UPPER({})", a[0]),
                ScalarFunc::Lower => format!("LOWER({})", a[0]),
                ScalarFunc::Len => format!("CHAR_LENGTH({})", a[0]),
                ScalarFunc::Left => format!("LEFT({}, {})", a[0], a.get(1).map_or("1", |n| n)),
                ScalarFunc::Right => format!("RIGHT({}, {})", a[0], a.get(1).map_or("1", |n| n)),
                ScalarFunc::Mid => format!("SUBSTRING({}, {}, {})", a[0], a[1], a[2]),
                ScalarFunc::Substitute if a.len() == 3 => {
                    format!("REPLACE({}, {}, {})", a[0], a[1], a[2])
                }
                // Blank handling (MIN/MAX), space collapsing (TRIM), date overflow and
                // format strings have no portable SQL spelling: no generated expression.
                ScalarFunc::Min
                | ScalarFunc::Max
                | ScalarFunc::Trim
                | ScalarFunc::Substitute
                | ScalarFunc::Format(_)
                | ScalarFunc::Date
                | ScalarFunc::EoMonth
                | ScalarFunc::EDate
                | ScalarFunc::DateDiff(_) => return None,
            }
        }
        Bound::Unary { op, expr } => {
            let e = sql(model, expr, stack)?;
            match op {
                UnaryOp::Neg => format!("-({e})"),
                UnaryOp::Plus => e,
                UnaryOp::Not => format!("(NOT {e})"),
            }
        }
        Bound::Related { .. }
        | Bound::Var(_)
        | Bound::Let { .. }
        | Bound::Aggregate { .. }
        | Bound::Calculate { .. }
        | Bound::Info { .. }
        | Bound::Switch { .. }
        | Bound::In { .. } => return None,
        Bound::Binary { op, left, right } => {
            let (l, r) = (sql(model, left, stack)?, sql(model, right, stack)?);
            let sym = match op {
                BinaryOp::Add => "+",
                BinaryOp::Sub => "-",
                BinaryOp::Mul => "*",
                BinaryOp::Div => "/",
                BinaryOp::Pow => return Some(format!("POWER({l}, {r})")),
                BinaryOp::Concat => "||",
                BinaryOp::Eq | BinaryOp::StrictEq => "=",
                BinaryOp::Ne => "<>",
                BinaryOp::Lt => "<",
                BinaryOp::Le => "<=",
                BinaryOp::Gt => ">",
                BinaryOp::Ge => ">=",
                BinaryOp::And => "AND",
                BinaryOp::Or => "OR",
                BinaryOp::In => return None,
            };
            format!("({l} {sym} {r})")
        }
    })
}
