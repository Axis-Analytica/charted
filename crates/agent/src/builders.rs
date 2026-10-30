//! The tables the UI's dialogs make, built the same way for the agent: what-if
//! parameters (ui/src/report/parameters.ts `parameterTable`), field parameters
//! (`fieldParameterTable`) and calculation groups (calcgroups.ts).

use charted_model::{
    CalculatedTable, CalculationGroup, Column, ColumnKind, DataType, ExpressionLanguage, Measure, Summarize, Table,
};

use crate::names::{quote_name, quote_table};

pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// `Table` as DAX writes it in a parameter's expressions: bare when it can be.
fn table_ref(name: &str) -> String {
    let bare = name.chars().next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_');
    if bare { name.to_string() } else { quote_table(name) }
}

fn data_column(id: String, name: &str, source: &str, data_type: DataType, hidden: bool) -> Column {
    Column {
        id,
        name: name.into(),
        data_type,
        kind: ColumnKind::Data {
            source_column: source.into(),
        },
        description: None,
        format_string: None,
        summarize_by: Summarize::None,
        data_category: None,
        sort_by: None,
        display_folder: None,
        hidden,
        is_key: false,
        extras: Default::default(),
    }
}

fn table(name: &str, columns: Vec<Column>, measures: Vec<Measure>, expression: String) -> Table {
    Table {
        id: new_id(),
        name: name.into(),
        description: None,
        source: String::new(),
        columns,
        measures,
        hierarchies: vec![],
        hidden: false,
        is_date_table: false,
        calculated: Some(CalculatedTable {
            expression,
            language: ExpressionLanguage::Dax,
        }),
        calculation_group: None,
        extras: Default::default(),
    }
}

/// A number as DAX writes it.
fn num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

/// A what-if parameter: GENERATESERIES values, and a measure of the value picked.
pub struct NumericParameter {
    pub name: String,
    pub decimal: bool,
    pub min: f64,
    pub max: f64,
    pub step: f64,
    pub default: f64,
}

impl NumericParameter {
    pub fn problem(&self) -> Option<&'static str> {
        let nums = [self.min, self.max, self.step, self.default];
        if nums.iter().any(|n| !n.is_finite()) {
            return Some("min, max, step and default must be numbers");
        }
        if !self.decimal && nums.iter().any(|n| n.fract() != 0.0) {
            return Some("a whole number parameter takes whole numbers (set decimal)");
        }
        if self.step <= 0.0 {
            return Some("the step must be more than 0");
        }
        if self.max < self.min {
            return Some("max must be at least min");
        }
        if (self.max - self.min) / self.step > 10_000.0 {
            return Some("that's more than 10,000 values; use a larger step");
        }
        if self.default < self.min || self.default > self.max {
            return Some("the default must be between min and max");
        }
        None
    }

    pub fn table(&self) -> Table {
        let decimals = if self.decimal {
            [self.min, self.step]
                .iter()
                .map(|n| num(*n).split('.').nth(1).map_or(0, str::len))
                .max()
                .unwrap_or(0)
        } else {
            0
        };
        let format = if self.decimal {
            format!("0.{}", "0".repeat(decimals.max(1)))
        } else {
            "0".into()
        };
        let mut column = data_column(
            new_id(),
            &self.name,
            "Value",
            if self.decimal { DataType::Decimal } else { DataType::Int64 },
            false,
        );
        column.format_string = Some(format.clone());
        let r = format!("{}{}", table_ref(&self.name), quote_name(&self.name));
        let measure = Measure {
            id: new_id(),
            name: format!("{} Value", self.name),
            expression: format!("SELECTEDVALUE ( {r}, {} )", num(self.default)),
            language: ExpressionLanguage::Dax,
            description: None,
            format_string: Some(format),
            display_folder: None,
            hidden: false,
            extras: Default::default(),
        };
        table(
            &self.name,
            vec![column],
            vec![measure],
            format!("GENERATESERIES ( {}, {}, {} )", num(self.min), num(self.max), num(self.step)),
        )
    }
}

fn text(s: &str) -> String {
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// A field parameter: rows of `( label, NAMEOF ( field ), order )`. `fields` are
/// (label, table name, field name).
pub fn field_parameter_table(name: &str, fields: &[(String, String, String)]) -> Table {
    let order = data_column(new_id(), &format!("{name} Order"), "Value3", DataType::Int64, true);
    let mut label = data_column(new_id(), name, "Value1", DataType::String, false);
    label.sort_by = Some(order.id.clone());
    let names = data_column(new_id(), &format!("{name} Fields"), "Value2", DataType::String, true);
    let rows: Vec<String> = fields
        .iter()
        .enumerate()
        .map(|(i, (l, t, f))| format!("    ( {}, NAMEOF ( {}{} ), {i} )", text(l), quote_table(t), quote_name(f)))
        .collect();
    table(name, vec![label, names, order], vec![], format!("{{\n{}\n}}", rows.join(",\n")))
}

/// A calculation group's table; the core gives it its Name and Ordinal columns.
pub fn calculation_group_table(name: &str, group: CalculationGroup) -> Table {
    let mut t = table(name, vec![], vec![], String::new());
    t.calculated = None;
    t.calculation_group = Some(group);
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    // The same DAX as ui/src/report/parameters.ts gives for these inputs.
    #[test]
    fn a_numeric_parameter_is_built_as_the_dialog_builds_it() {
        let p = NumericParameter {
            name: "Discount %".into(),
            decimal: true,
            min: 0.0,
            max: 0.5,
            step: 0.05,
            default: 0.1,
        };
        assert_eq!(p.problem(), None);
        let t = p.table();
        assert_eq!(t.calculated.unwrap().expression, "GENERATESERIES ( 0, 0.5, 0.05 )");
        assert_eq!(t.measures[0].name, "Discount % Value");
        assert_eq!(t.measures[0].expression, "SELECTEDVALUE ( 'Discount %'[Discount %], 0.1 )");
        assert_eq!(t.columns[0].format_string.as_deref(), Some("0.00"));
        let whole = NumericParameter { name: "N".into(), decimal: false, min: 1.0, max: 10.0, step: 1.0, default: 5.0 };
        assert_eq!(whole.table().measures[0].expression, "SELECTEDVALUE ( N[N], 5 )");
        assert!(NumericParameter { step: 0.0, ..whole }.problem().is_some());
    }

    #[test]
    fn a_field_parameter_is_built_as_the_dialog_builds_it() {
        let t = field_parameter_table(
            "Metric",
            &[
                ("Sales".into(), "Sales".into(), "Total Sales".into()),
                ("Units".into(), "Sales".into(), "Units".into()),
            ],
        );
        assert_eq!(
            t.calculated.unwrap().expression,
            "{\n    ( \"Sales\", NAMEOF ( 'Sales'[Total Sales] ), 0 ),\n    ( \"Units\", NAMEOF ( 'Sales'[Units] ), 1 )\n}"
        );
        assert_eq!(t.columns[0].sort_by.as_deref(), Some(t.columns[2].id.as_str()));
        assert!(t.columns[1].hidden && t.columns[2].hidden);
    }
}
