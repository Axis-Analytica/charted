//! Generates the "Harbor & Pine Outfitters" sample: a fictional outdoor retailer with a
//! sales fact table and date, product, customer and store dimensions, plus monthly
//! store targets at a coarser grain.
//!
//! Output is fully deterministic for a given scale (ChaCha8, seed 42), so screenshots,
//! tests and documentation always show the same numbers. The data is synthetic and
//! released under CC0.
//!
//! Built-in patterns, so reports have something to find:
//! - about 8% year-on-year growth and a rising online share
//! - a November/December peak, plus a summer peak for camping and footwear
//! - weekend uplift in stores and weekday uplift online
//! - a 25% promotion through March 2025
//! - the "Old Mill" store closing at the end of June 2024
//! - the "Ridgeline" trail-running line launching in June 2024
//! - about 1% of lines are returns (negative quantity)

mod calendar;
mod names;

use duckdb::{Connection, params};
use rand::{RngExt, SeedableRng};
use rand_chacha::ChaCha8Rng;
use std::path::{Path, PathBuf};

use calendar::{civil_from_days, days_from_civil, weekday};
use names::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scale {
    /// About 250 thousand sales lines.
    Demo,
    /// About 1 million sales lines.
    Million,
    /// About 10 million sales lines.
    TenMillion,
}

impl Scale {
    fn factor(self) -> f64 {
        match self {
            Scale::Demo => 1.0,
            Scale::Million => 4.0,
            Scale::TenMillion => 40.0,
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "demo" => Some(Scale::Demo),
            "1m" => Some(Scale::Million),
            "10m" => Some(Scale::TenMillion),
            _ => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SampleError {
    #[error("DuckDB error while generating the sample: {0}")]
    Db(#[from] duckdb::Error),
    #[error("could not create {path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

/// One generated data file.
#[derive(Debug, Clone)]
pub struct SampleTable {
    pub name: &'static str,
    pub file: PathBuf,
    pub rows: u64,
}

pub const FIRST_DAY: (i32, u32, u32) = (2022, 1, 1);
pub const LAST_DAY: (i32, u32, u32) = (2025, 12, 31);
const SEED: u64 = 42;

/// Writes the sample data files into `dir` (created if needed): the sales fact as
/// Parquet and the dimensions as CSV, to exercise both loaders.
pub fn generate(dir: &Path, scale: Scale) -> Result<Vec<SampleTable>, SampleError> {
    std::fs::create_dir_all(dir).map_err(|source| SampleError::Io {
        path: dir.to_path_buf(),
        source,
    })?;
    let conn = Connection::open_in_memory()?;
    // One thread keeps file output byte-identical between runs.
    conn.execute_batch("SET threads = 1; SET preserve_insertion_order = true;")?;
    let mut rng = ChaCha8Rng::seed_from_u64(SEED);

    create_date(&conn)?;
    let products = create_products(&conn, &mut rng)?;
    let customers = create_customers(&conn, &mut rng, 20_000)?;
    let stores = create_stores(&conn, &mut rng)?;
    create_sales(&conn, &mut rng, &products, customers, &stores, scale)?;
    create_targets(&conn, &stores)?;

    let mut out = Vec::new();
    for (name, file, format) in [
        (
            "Sales",
            "sales.parquet",
            "(FORMAT parquet, COMPRESSION zstd, ROW_GROUP_SIZE 122880)",
        ),
        ("Date", "date.csv", "(FORMAT csv, HEADER true)"),
        ("Product", "product.csv", "(FORMAT csv, HEADER true)"),
        ("Customer", "customer.csv", "(FORMAT csv, HEADER true)"),
        ("Store", "store.csv", "(FORMAT csv, HEADER true)"),
        ("Targets", "targets.csv", "(FORMAT csv, HEADER true)"),
    ] {
        let path = dir.join(file);
        let table = name.to_lowercase();
        conn.execute_batch(&format!(
            "COPY \"{table}\" TO '{}' {format}",
            path.display().to_string().replace('\'', "''")
        ))?;
        let rows: u64 = conn.query_row(&format!("SELECT count(*) FROM \"{table}\""), [], |r| {
            r.get(0)
        })?;
        out.push(SampleTable {
            name,
            file: path,
            rows,
        });
    }
    write_stores_raw(&conn, dir)?;
    write_targets_workbook(&conn, dir)?;
    Ok(out)
}

/// The messy inputs for practising in the query editor (not loaded by the sample):
/// a store list exported from a "property register", with title rows above the
/// header, padded and lower-case names, "City, Country" in one column, numbers with
/// thousands separators, blank rows, a duplicated store and inconsistent formats.
pub const STORES_RAW: &str = "stores_raw.csv";
/// Monthly targets as a workbook: one sheet per year plus a notes sheet.
pub const TARGETS_WORKBOOK: &str = "targets.xlsx";

fn write_stores_raw(conn: &Connection, dir: &Path) -> Result<(), SampleError> {
    let mut stmt = conn.prepare(
        "SELECT StoreCode, Store, City, Country, SquareFeet, StoreKey FROM store WHERE Channel = 'Store' ORDER BY StoreKey",
    )?;
    let rows: Vec<(String, String, String, String, i32, i32)> = stmt
        .query_map([], |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
            ))
        })?
        .collect::<Result<_, _>>()?;
    let managers = [
        "Ana Ruiz",
        "Ben Okafor",
        "Chloe Martin",
        "Dev Patel",
        "Erin Walsh",
        "Felix Braun",
        "Grace Lee",
        "Hugo Silva",
        "Ines Novak",
        "Jonah Reid",
    ];
    let mut text = String::from(
        "Harbor & Pine property register,,,,,\nExported 2025-12-31 by Facilities; do not edit,,,,,\nstore code,store name,location,manager,format,square feet\n",
    );
    for (i, (code, name, city, country, sqft, key)) in rows.iter().enumerate() {
        let manager = if key % 7 == 0 {
            ""
        } else {
            managers[*key as usize % managers.len()]
        };
        let format = match (sqft, key % 3) {
            (s, 0) if *s >= 24000 => "FLAGSHIP",
            (s, _) if *s >= 24000 => "Flagship",
            (s, 1) if *s < 12000 => "outlet",
            (s, _) if *s < 12000 => "Outlet",
            (_, 2) => "standard ",
            _ => "Standard",
        };
        let shown = if key % 2 == 0 {
            name.to_lowercase()
        } else {
            format!("  {name} ")
        };
        let line = format!(
            "{code},{shown},\"{city}, {country}\",{manager},{format},\"{}\"\n",
            thousands(*sqft)
        );
        text.push_str(&line);
        if i == 5 {
            // A store exported twice, and an empty line.
            text.push_str(&line);
            text.push_str(",,,,,\n");
        }
    }
    text.push_str(",,,,,\n");
    let path = dir.join(STORES_RAW);
    std::fs::write(&path, text).map_err(|source| SampleError::Io { path, source })
}

fn thousands(n: i32) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

fn write_targets_workbook(conn: &Connection, dir: &Path) -> Result<(), SampleError> {
    use rust_xlsxwriter::{DocProperties, ExcelDateTime, Format, Workbook};
    let mut wb = Workbook::new();
    // A fixed creation date keeps the file identical between runs.
    if let Ok(created) = ExcelDateTime::from_ymd(2025, 12, 31) {
        wb.set_properties(&DocProperties::new().set_creation_datetime(&created));
    }
    let date = Format::new().set_num_format("yyyy-mm-dd");
    let bold = Format::new().set_bold();
    for year in [2024, 2025] {
        let sheet = wb.add_worksheet();
        let _ = sheet.set_name(format!("Targets {year}"));
        let _ = sheet.write_string_with_format(0, 0, "Store Code", &bold);
        let _ = sheet.write_string_with_format(0, 1, "Month", &bold);
        let _ = sheet.write_string_with_format(0, 2, "Target", &bold);
        let mut stmt = conn.prepare(&format!(
            "SELECT s.StoreCode, t.DateKey, CAST(t.TargetAmount AS DOUBLE) FROM targets t JOIN store s USING (StoreKey) WHERE t.DateKey // 10000 = {year} ORDER BY s.StoreCode, t.DateKey"
        ))?;
        let rows: Vec<(String, i32, f64)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        for (i, (code, key, amount)) in rows.iter().enumerate() {
            let r = i as u32 + 1;
            let _ = sheet.write_string(r, 0, code);
            if let Ok(d) = ExcelDateTime::from_ymd(
                (key / 10000) as u16,
                (key / 100 % 100) as u8,
                (key % 100) as u8,
            ) {
                let _ = sheet.write_datetime_with_format(r, 1, &d, &date);
            }
            let _ = sheet.write_number(r, 2, *amount);
        }
    }
    let notes = wb.add_worksheet();
    let _ = notes.set_name("Notes");
    let _ = notes.write_string(
        0,
        0,
        "Targets are 5% above the previous year's sales, rounded to the nearest 1,000.",
    );
    let path = dir.join(TARGETS_WORKBOOK);
    wb.save(&path).map_err(|e| SampleError::Io {
        path,
        source: std::io::Error::other(e.to_string()),
    })
}

fn create_date(conn: &Connection) -> duckdb::Result<()> {
    conn.execute_batch(
        "CREATE TABLE date AS
         SELECT CAST(strftime(d, '%Y%m%d') AS INTEGER) AS DateKey,
                CAST(d AS DATE) AS Date,
                year(d) AS Year,
                'Q' || quarter(d) AS Quarter,
                month(d) AS MonthNumber,
                monthname(d) AS MonthName,
                strftime(d, '%Y-%m') AS YearMonth,
                dayname(d) AS Weekday,
                isodow(d) AS WeekdayNumber,
                isodow(d) >= 6 AS IsWeekend,
                'FY' || (year(d) + CASE WHEN month(d) >= 7 THEN 1 ELSE 0 END) AS FiscalYear
         FROM generate_series(DATE '2022-01-01', DATE '2025-12-31', INTERVAL 1 DAY) AS t(d)
         ORDER BY d",
    )
}

struct Product {
    key: i32,
    category: usize,
    price: f64,
    cost: f64,
    /// Day number from which the product sells.
    launch: i64,
}

fn pick<'a, T>(rng: &mut ChaCha8Rng, items: &'a [T]) -> &'a T {
    &items[rng.random_range(0..items.len())]
}

fn weighted(rng: &mut ChaCha8Rng, weights: &[f64]) -> usize {
    let total: f64 = weights.iter().sum();
    let mut x = rng.random::<f64>() * total;
    for (i, w) in weights.iter().enumerate() {
        if x < *w {
            return i;
        }
        x -= w;
    }
    weights.len() - 1
}

fn round2(x: f64) -> f64 {
    (x * 100.0).round() / 100.0
}

fn date_str(day: i64) -> String {
    let (y, m, d) = civil_from_days(day);
    format!("{y:04}-{m:02}-{d:02}")
}

fn date_key(day: i64) -> i32 {
    let (y, m, d) = civil_from_days(day);
    y * 10000 + m as i32 * 100 + d as i32
}

fn create_products(conn: &Connection, rng: &mut ChaCha8Rng) -> duckdb::Result<Vec<Product>> {
    conn.execute_batch(
        "CREATE TABLE product (ProductKey INTEGER, Product VARCHAR, Category VARCHAR, Subcategory VARCHAR,
         Brand VARCHAR, Color VARCHAR, ListPrice DECIMAL(10,2), UnitCost DECIMAL(10,2), LaunchDate DATE)",
    )?;
    let launch_ridgeline = days_from_civil(2024, 6, 1);
    let mut products = Vec::new();
    let mut app = conn.appender("product")?;
    let mut key = 1;
    for (ci, (category, subs)) in CATEGORIES.iter().enumerate() {
        for sub in subs.iter() {
            let band = match *category {
                "Camping" => 60.0..420.0,
                "Outerwear" => 70.0..380.0,
                "Footwear" => 60.0..220.0,
                "Packs" => 35.0..260.0,
                "Apparel" => 15.0..110.0,
                _ => 8.0..90.0,
            };
            let count = rng.random_range(20..34);
            for i in 0..count {
                let ridgeline = *sub == "Trail Running" && i < 6;
                let word = if ridgeline {
                    "Ridgeline"
                } else {
                    pick(rng, PRODUCT_WORDS)
                };
                let color = pick(rng, COLORS);
                let name = format!("{word} {} {}", sub.trim_end_matches('s'), i + 1);
                let price = (rng.random_range(band.clone()) / 5.0_f64).round() * 5.0 - 0.01;
                let cost = round2(price * rng.random_range(0.38..0.62));
                let launch = if ridgeline {
                    launch_ridgeline
                } else {
                    days_from_civil(2020, 1, 1) + rng.random_range(0..900)
                };
                let brand = if ridgeline {
                    "Harbor & Pine"
                } else {
                    *pick(rng, BRANDS)
                };
                app.append_row(params![
                    key,
                    name,
                    category,
                    sub,
                    brand,
                    color,
                    price,
                    cost,
                    date_str(launch)
                ])?;
                products.push(Product {
                    key,
                    category: ci,
                    price,
                    cost,
                    launch,
                });
                key += 1;
            }
        }
    }
    app.flush()?;
    Ok(products)
}

fn create_customers(conn: &Connection, rng: &mut ChaCha8Rng, n: i32) -> duckdb::Result<i32> {
    conn.execute_batch(
        "CREATE TABLE customer (CustomerKey INTEGER, Customer VARCHAR, Segment VARCHAR, City VARCHAR,
         Country VARCHAR, Region VARCHAR, JoinDate DATE, BirthYear INTEGER)",
    )?;
    let mut app = conn.appender("customer")?;
    let seg_weights: Vec<f64> = SEGMENTS.iter().map(|s| s.1).collect();
    for key in 1..=n {
        let first = format!("{}{}", pick(rng, FIRST_SYLLABLES), pick(rng, FIRST_ENDINGS));
        let last = format!("{}{}", pick(rng, LAST_STARTS), pick(rng, LAST_ENDS));
        let (city, country, region) = *pick(rng, CITIES);
        let segment = SEGMENTS[weighted(rng, &seg_weights)].0;
        let join = days_from_civil(2018, 1, 1) + rng.random_range(0..2900);
        let birth = rng.random_range(1950..2006);
        app.append_row(params![
            key,
            format!("{first} {last}"),
            segment,
            city,
            country,
            region,
            date_str(join),
            birth
        ])?;
    }
    app.flush()?;
    Ok(n)
}

struct Store {
    key: i32,
    online: bool,
    weight: f64,
    open: i64,
    close: Option<i64>,
}

fn create_stores(conn: &Connection, rng: &mut ChaCha8Rng) -> duckdb::Result<Vec<Store>> {
    conn.execute_batch(
        "CREATE TABLE store (StoreKey INTEGER, StoreCode VARCHAR, Store VARCHAR, City VARCHAR, Country VARCHAR,
         Region VARCHAR, Channel VARCHAR, OpenDate DATE, CloseDate DATE, SquareFeet INTEGER)",
    )?;
    let mut app = conn.appender("store")?;
    let mut stores = Vec::new();
    let streets = [
        "Harbor St",
        "Main St",
        "Market Sq",
        "Pine Ridge",
        "Riverside",
        "Old Mill",
        "Northgate",
        "Station Rd",
    ];
    let mut used = std::collections::BTreeSet::new();
    let mut key = 1;
    while stores.len() < 40 {
        let (city, country, region) = *pick(rng, CITIES);
        let street = *pick(rng, &streets);
        if !used.insert((city, street)) {
            continue;
        }
        let name = format!("{city} {street}");
        let sqft = rng.random_range(8..32) * 1000;
        // A handful of stores open during the period, so their sales start later.
        let open = if key % 9 == 0 {
            days_from_civil(2023, 3, 1) + rng.random_range(0..400)
        } else {
            days_from_civil(2012, 1, 1) + rng.random_range(0..3000)
        };
        // The first "Old Mill" store closes mid-2024.
        let close = (street == "Old Mill" && !stores.iter().any(|s: &Store| s.close.is_some()))
            .then(|| days_from_civil(2024, 6, 30));
        app.append_row(params![
            key,
            format!("HP-{key:03}"),
            name,
            city,
            country,
            region,
            "Store",
            date_str(open),
            close.map(date_str),
            sqft
        ])?;
        stores.push(Store {
            key,
            online: false,
            weight: sqft as f64,
            open,
            close,
        });
        key += 1;
    }
    app.append_row(params![
        key,
        "HP-WEB",
        "Online",
        None::<String>,
        None::<String>,
        "Online",
        "Online",
        "2015-04-01",
        None::<String>,
        None::<i32>
    ])?;
    stores.push(Store {
        key,
        online: true,
        weight: 0.0,
        open: days_from_civil(2015, 4, 1),
        close: None,
    });
    app.flush()?;
    Ok(stores)
}

/// Relative demand by month (index 0 = January) and category.
fn season(category: &str, month: u32) -> f64 {
    let m = month as usize - 1;
    let base = [
        0.80, 0.75, 0.90, 0.95, 1.00, 1.05, 1.05, 1.00, 0.95, 1.00, 1.30, 1.65,
    ][m];
    let shape = match category {
        "Camping" => [0.6, 0.6, 0.8, 1.0, 1.3, 1.6, 1.7, 1.5, 1.0, 0.7, 0.8, 1.0][m],
        "Footwear" => [0.8, 0.8, 1.0, 1.1, 1.2, 1.3, 1.3, 1.2, 1.0, 0.9, 0.9, 1.0][m],
        "Outerwear" => [1.3, 1.1, 0.9, 0.7, 0.5, 0.4, 0.4, 0.5, 0.8, 1.2, 1.5, 1.6][m],
        _ => 1.0,
    };
    base * shape
}

fn create_sales(
    conn: &Connection,
    rng: &mut ChaCha8Rng,
    products: &[Product],
    customers: i32,
    stores: &[Store],
    scale: Scale,
) -> duckdb::Result<()> {
    conn.execute_batch(
        "CREATE TABLE sales_raw (OrderNumber VARCHAR, LineNumber INTEGER, OrderDateKey INTEGER, ShipDateKey INTEGER,
         ProductKey INTEGER, CustomerKey INTEGER, StoreKey INTEGER, Channel VARCHAR, Quantity INTEGER,
         UnitPrice DOUBLE, Discount DOUBLE, UnitCost DOUBLE)",
    )?;
    let first = days_from_civil(FIRST_DAY.0, FIRST_DAY.1, FIRST_DAY.2);
    let last = days_from_civil(LAST_DAY.0, LAST_DAY.1, LAST_DAY.2);
    let promo = (days_from_civil(2025, 3, 1), days_from_civil(2025, 3, 31));
    let base_orders = 78.0 * scale.factor();
    let mut by_category: Vec<Vec<&Product>> = vec![Vec::new(); CATEGORIES.len()];
    for p in products {
        by_category[p.category].push(p);
    }
    let online = stores.iter().find(|s| s.online).expect("online store");
    let mut app = conn.appender("sales_raw")?;
    let mut order_no: u64 = 100_000;
    for day in first..=last {
        let (_, month, _) = civil_from_days(day);
        let years = (day - first) as f64 / 365.25;
        let growth = 1.08f64.powf(years);
        let wd = weekday(day); // 1 = Monday … 7 = Sunday
        let weekend = wd >= 6;
        let online_share = 0.22 + 0.04 * years;
        let expected = base_orders * growth * season("", month);
        let orders = (expected * rng.random_range(0.85..1.15)).round() as u64;
        let open_stores: Vec<&Store> = stores
            .iter()
            .filter(|s| !s.online && s.open <= day && s.close.is_none_or(|c| day <= c))
            .collect();
        let weights: Vec<f64> = open_stores.iter().map(|s| s.weight).collect();
        let cat_weights: Vec<f64> = CATEGORIES.iter().map(|(c, _)| season(c, month)).collect();
        for _ in 0..orders {
            let channel_online =
                rng.random::<f64>() < online_share * if weekend { 0.75 } else { 1.12 };
            let store = if channel_online {
                online
            } else {
                open_stores[weighted(rng, &weights)]
            };
            // Stores are busier at weekends: thin out weekday store orders a little.
            if !channel_online && !weekend && rng.random::<f64>() < 0.12 {
                continue;
            }
            order_no += 1;
            let customer = rng.random_range(1..=customers);
            let ship = if channel_online {
                day + rng.random_range(1..6)
            } else {
                day
            };
            let lines = 1 + weighted(rng, &[0.45, 0.30, 0.17, 0.08]) as i32;
            for line in 1..=lines {
                let cat = weighted(rng, &cat_weights);
                let candidates = &by_category[cat];
                let p = loop {
                    let p = *pick(rng, candidates);
                    if p.launch <= day {
                        break p;
                    }
                };
                let mut qty = 1 + weighted(rng, &[0.7, 0.2, 0.07, 0.03]) as i32;
                if rng.random::<f64>() < 0.01 {
                    qty = -qty;
                }
                let discount = if day >= promo.0 && day <= promo.1 && rng.random::<f64>() < 0.6 {
                    0.25
                } else if rng.random::<f64>() < 0.08 {
                    0.10
                } else {
                    0.0
                };
                app.append_row(params![
                    format!("SO{order_no}"),
                    line,
                    date_key(day),
                    date_key(ship.min(last)),
                    p.key,
                    customer,
                    store.key,
                    if channel_online { "Online" } else { "Store" },
                    qty,
                    p.price,
                    discount,
                    p.cost
                ])?;
            }
        }
    }
    app.flush()?;
    drop(app);
    conn.execute_batch(
        "CREATE TABLE sales AS
         SELECT OrderNumber, LineNumber, OrderDateKey, ShipDateKey, ProductKey, CustomerKey, StoreKey, Channel,
                Quantity, CAST(UnitPrice AS DECIMAL(10,2)) AS UnitPrice, CAST(Discount AS DECIMAL(4,2)) AS Discount,
                CAST(UnitCost AS DECIMAL(10,2)) AS UnitCost,
                CAST(round(Quantity * UnitPrice * (1 - Discount), 2) AS DECIMAL(12,2)) AS Amount,
                CAST(round(Quantity * UnitCost, 2) AS DECIMAL(12,2)) AS Cost
         FROM sales_raw;
         DROP TABLE sales_raw;",
    )
}

/// Monthly sales targets per physical store, keyed by the first day of the month.
fn create_targets(conn: &Connection, stores: &[Store]) -> duckdb::Result<()> {
    let physical: Vec<String> = stores
        .iter()
        .filter(|s| !s.online)
        .map(|s| s.key.to_string())
        .collect();
    conn.execute_batch(&format!(
        "CREATE TABLE targets AS
         SELECT CAST(strftime(date_trunc('month', d.Date), '%Y%m%d') AS INTEGER) AS DateKey,
                s.StoreKey,
                CAST(round(sum(s.Amount) * 1.05 / 1000) * 1000 AS DECIMAL(12,0)) AS TargetAmount
         FROM sales s JOIN date d ON d.DateKey = s.OrderDateKey
         WHERE s.StoreKey IN ({})
         GROUP BY ALL
         ORDER BY 1, 2",
        physical.join(", ")
    ))
}
