//! Awkward real-world files through Get Data: odd encodings and delimiters, quoting,
//! ragged rows, bad headers, empty files, wide and long files, messy workbooks,
//! nested JSON and unusual Parquet types. Each either loads the right values or fails
//! with a message that says what is wrong; none panics or loads wrong data silently.

use arrow_ipc::reader::StreamReader;
use charted_compiler::steps::*;
use charted_core::queryedit::QueryInfo;
use charted_core::{App, DataRequest, Request};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn rows(bytes: Vec<u8>) -> Vec<Vec<String>> {
    let reader = StreamReader::try_new(std::io::Cursor::new(bytes), None).unwrap();
    let mut out = Vec::new();
    for batch in reader {
        let batch = batch.unwrap();
        let fmts: Vec<_> = batch
            .columns()
            .iter()
            // (Arrow's formatter here knows no named time zones; the UI's does.)
            .map(|c| arrow_cast::display::ArrayFormatter::try_new(c, &Default::default()).ok())
            .collect();
        for r in 0..batch.num_rows() {
            out.push(
                fmts.iter()
                    .map(|f| {
                        f.as_ref()
                            .map_or("(zoned time)".into(), |f| f.value(r).to_string())
                    })
                    .collect(),
            );
        }
    }
    out
}

struct Fx {
    app: App,
    dir: tempfile::TempDir,
}

/// What a file loaded as: column names and types, and its first rows.
#[derive(Debug)]
struct Loaded {
    columns: Vec<(String, String)>,
    rows: Vec<Vec<String>>,
    /// The loaded table's column types.
    loaded: Vec<charted_model::DataType>,
}

impl Fx {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path().join("state"));
        app.handle(Request::NewProject {
            name: "Torture".into(),
            dir: Some(dir.path().display().to_string()),
        })
        .unwrap();
        Self { app, dir }
    }

    fn file(&self, name: &str, bytes: impl AsRef<[u8]>) -> PathBuf {
        let p = self.dir.path().join(name);
        std::fs::write(&p, bytes).unwrap();
        p
    }

    /// Previews the source, then loads it as a table.
    fn load(&mut self, source: Source) -> Result<Loaded, String> {
        let q = Query {
            id: "q".into(),
            name: "T".into(),
            table: "q".into(),
            source,
            steps: vec![],
            step_names: BTreeMap::new(),
            no_load: false,
        };
        let info: QueryInfo = serde_json::from_slice(
            &self
                .app
                .data(DataRequest::QueryInfo {
                    query: q.clone(),
                    queries: vec![],
                    upto: 0,
                })
                .map_err(|e| e.message)?,
        )
        .unwrap();
        let rows = rows(
            self.app
                .data(DataRequest::QueryPreview {
                    query: q.clone(),
                    queries: vec![],
                    upto: 0,
                    limit: 100,
                })
                .map_err(|e| e.message)?,
        );
        self.app
            .handle(Request::ApplyQueries { queries: vec![q] })
            .map_err(|e| e.message)?;
        let model = &self.app.session().unwrap().project.model;
        let table = model.tables.iter().find(|t| t.name == "T").unwrap();
        Ok(Loaded {
            loaded: table.columns.iter().map(|c| c.data_type).collect(),
            columns: info
                .columns
                .into_iter()
                .map(|c| (c.name, c.duckdb_type))
                .collect(),
            rows,
        })
    }
}

fn csv(p: &Path) -> Source {
    Source::Csv {
        path: p.display().to_string(),
        delimiter: None,
        header: true,
        encoding: None,
        decimal: None,
    }
}

fn utf16le(s: &str) -> Vec<u8> {
    let mut out = vec![0xFF, 0xFE];
    for u in s.encode_utf16() {
        out.extend_from_slice(&u.to_le_bytes());
    }
    out
}

fn latin1(s: &str) -> Vec<u8> {
    s.chars().map(|c| c as u32 as u8).collect()
}

#[test]
fn awkward_files_load_right_or_say_why() {
    let mut fx = Fx::new();
    let mut cases: Vec<(&str, Source)> = Vec::new();
    let f = |fx: &Fx, n: &str, b: Vec<u8>| csv(&fx.file(n, b));
    cases.push((
        "utf8 bom",
        f(
            &fx,
            "bom.csv",
            [
                &[0xEF, 0xBB, 0xBF][..],
                "Name,Amount\nCafé,1.5\nZoë,2\n".as_bytes(),
            ]
            .concat(),
        ),
    ));
    cases.push((
        "utf16le",
        f(
            &fx,
            "u16.csv",
            utf16le("Name,Amount\r\nCafé,1.5\r\nZoë,2\r\n"),
        ),
    ));
    cases.push((
        "latin1",
        f(&fx, "l1.csv", latin1("Name,Amount\nCafé,1.5\nZoë,2\n")),
    ));
    cases.push((
        "semicolon decimal comma",
        f(&fx, "semi.csv", b"Name;Amount\nA;1,5\nB;2,25\n".to_vec()),
    ));
    cases.push((
        "pipe",
        f(&fx, "pipe.csv", b"Name|Amount\nA|1\nB|2\n".to_vec()),
    ));
    cases.push((
        "tab",
        f(&fx, "tab.csv", b"Name\tAmount\nA\t1\nB\t2\n".to_vec()),
    ));
    cases.push((
        "quoted newline",
        f(
            &fx,
            "q.csv",
            b"Name,Note\nA,\"line one\nline two\"\nB,\"say \"\"hi\"\"\"\n".to_vec(),
        ),
    ));
    cases.push((
        "ragged",
        f(
            &fx,
            "rag.csv",
            b"A,B,C\n1,2,3\n4,5\n6,7,8,9\n10,11,12\n".to_vec(),
        ),
    ));
    cases.push((
        "dup and blank headers",
        f(&fx, "dup.csv", b"Name,Name,,Amount\nA,B,x,1\n".to_vec()),
    ));
    cases.push(("header only", f(&fx, "head.csv", b"Name,Amount\n".to_vec())));
    cases.push(("empty", f(&fx, "empty.csv", b"".to_vec())));
    cases.push((
        "crlf trailing delim",
        f(&fx, "crlf.csv", b"A,B,\r\n1,2,\r\n3,4,\r\n".to_vec()),
    ));
    cases.push((
        "thousands",
        f(
            &fx,
            "thou.csv",
            b"Name,Amount\nA,\"1,234.50\"\nB,\"12,000\"\n".to_vec(),
        ),
    ));
    cases.push((
        "dmy dates",
        f(
            &fx,
            "dmy.csv",
            b"Date,X\n13/01/2024,1\n31/12/2024,2\n01/02/2024,3\n".to_vec(),
        ),
    ));
    cases.push((
        "mdy dates",
        f(
            &fx,
            "mdy.csv",
            b"Date,X\n01/13/2024,1\n12/31/2024,2\n".to_vec(),
        ),
    ));
    cases.push((
        "iso dates",
        f(
            &fx,
            "iso.csv",
            b"Date,X\n2024-01-13,1\n2024-12-31T10:00:00,2\n".to_vec(),
        ),
    ));
    let wide: String = (0..500)
        .map(|i| format!("c{i}"))
        .collect::<Vec<_>>()
        .join(",")
        + "\n"
        + &(0..500)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(",")
        + "\n";
    cases.push(("500 columns", f(&fx, "wide.csv", wide.into_bytes())));
    cases.push((
        "mixed types late",
        f(
            &fx,
            "late.csv",
            format!(
                "Id,Code\n{}9999,ABC\n",
                (0..30000).map(|i| format!("{i},{i}\n")).collect::<String>()
            )
            .into_bytes(),
        ),
    ));
    cases.push((
        "title rows",
        f(
            &fx,
            "title.csv",
            b"Sales report\nGenerated 2024-01-01\n\nName,Amount\nA,1\n".to_vec(),
        ),
    ));
    // JSON
    let j = |fx: &Fx, n: &str, s: &str| Source::Json {
        path: fx.file(n, s).display().to_string(),
    };
    cases.push(("json nested", j(&fx, "n.json", r#"[{"id":1,"customer":{"name":"A","city":"X"},"tags":["a","b"]},{"id":2,"customer":{"name":"B"},"tags":[]}]"#)));
    cases.push((
        "ndjson uneven",
        j(
            &fx,
            "n.ndjson",
            "{\"a\":1}\n{\"a\":2,\"b\":\"x\"}\n{\"b\":\"y\"}\n",
        ),
    ));
    cases.push((
        "json object",
        j(&fx, "o.json", r#"{"data":[{"a":1},{"a":2}],"count":2}"#),
    ));
    cases.push(("json bad", j(&fx, "bad.json", "{not json")));
    // Workbooks
    {
        use rust_xlsxwriter::{Format, Formula, Workbook};
        let mut wb = Workbook::new();
        let date = Format::new().set_num_format("yyyy-mm-dd");
        let s = wb.add_worksheet().set_name("Data").unwrap();
        s.write_row(0, 0, ["Region", "Amount", "When", "Ratio"])
            .unwrap();
        for r in 1..=5u32 {
            s.write(r, 0, format!("R{r}")).unwrap();
            s.write(r, 1, f64::from(r) * 1.5).unwrap();
            s.write_with_format(r, 2, 45000.0 + f64::from(r), &date)
                .unwrap();
            s.write_formula(
                r,
                3,
                Formula::new(format!("=B{}/{}", r + 1, if r == 3 { 0 } else { 2 }))
                    .set_result(if r == 3 { "#DIV/0!" } else { "1" }),
            )
            .unwrap();
        }
        wb.add_worksheet().set_name("Empty").unwrap();
        let t = wb.add_worksheet().set_name("Report").unwrap();
        t.merge_range(0, 0, 0, 2, "Quarterly report", &Format::new())
            .unwrap();
        t.write(1, 0, "Prepared by finance").unwrap();
        t.write_row(3, 0, ["Product", "Q1", "Q2"]).unwrap();
        t.write_row(4, 0, ["Bikes", "10", "12"]).unwrap();
        t.write(5, 0, "Helmets").unwrap();
        t.write(5, 1, 3).unwrap();
        t.write(5, 2, 4).unwrap();
        let m = wb.add_worksheet().set_name("Merged header").unwrap();
        m.merge_range(0, 1, 0, 2, "Sales", &Format::new()).unwrap();
        m.write_row(1, 0, ["Region", "2023", "2024"]).unwrap();
        m.write(2, 0, "North").unwrap();
        m.write(2, 1, 1).unwrap();
        m.write(2, 2, 2).unwrap();
        let p = fx.dir.path().join("book.xlsx");
        wb.save(&p).unwrap();
        for sheet in ["Data", "Empty", "Report", "Merged header", "Missing"] {
            cases.push((
                Box::leak(format!("xlsx {sheet}").into_boxed_str()),
                Source::Excel {
                    path: p.display().to_string(),
                    sheet: sheet.into(),
                    header: true,
                },
            ));
        }
        cases.push((
            "xlsx not a workbook",
            Source::Excel {
                path: fx.file("fake.xlsx", "hello").display().to_string(),
                sheet: "Sheet1".into(),
                header: true,
            },
        ));
    }
    // Parquet with decimals, time zones, nested structs and long strings.
    {
        let db = charted_engine::Database::open(None, &Default::default()).unwrap();
        let p = fx.dir.path().join("types.parquet");
        db.execute(&format!(
            "COPY (SELECT 1.25::DECIMAL(18,4) AS price, TIMESTAMPTZ '2024-03-01 10:00:00+05' AS at, {{'a': 1, 'b': 'x'}} AS rec, repeat('long ', 20000) AS note, [1, 2] AS list, 'é'::VARCHAR AS accent) TO '{}' (FORMAT parquet)",
            p.display()
        ))
        .unwrap();
        cases.push((
            "parquet types",
            Source::Parquet {
                path: p.display().to_string(),
            },
        ));
        cases.push((
            "parquet broken",
            Source::Parquet {
                path: fx
                    .file("broken.parquet", "PAR1 not really")
                    .display()
                    .to_string(),
            },
        ));
        cases.push(("missing file", csv(&fx.dir.path().join("nope.csv"))));
    }
    let mut results = std::collections::HashMap::new();
    for (name, source) in cases {
        let r = fx.load(source);
        if std::env::var_os("CHARTED_SURVEY").is_some() {
            println!("{name:28} {r:?}");
        }
        results.insert(name, r);
    }
    let ok = |n: &str| -> &Loaded {
        match &results[n] {
            Ok(l) => l,
            Err(e) => panic!("{n}: {e}"),
        }
    };
    let err = |n: &str| -> &str {
        match &results[n] {
            Ok(l) => panic!("{n} loaded: {l:?}"),
            Err(e) => e,
        }
    };
    let names = |l: &Loaded| {
        l.columns
            .iter()
            .map(|c| c.0.as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    let types = |l: &Loaded| {
        l.columns
            .iter()
            .map(|c| c.1.as_str())
            .collect::<Vec<_>>()
            .join(",")
    };
    let cell = |l: &Loaded, r: usize, c: usize| l.rows[r][c].clone();

    // Encodings: the same names and numbers whatever the file's encoding.
    for n in ["utf8 bom", "utf16le", "latin1"] {
        let l = ok(n);
        assert_eq!(names(l), "Name,Amount", "{n}");
        assert_eq!(types(l), "VARCHAR,DOUBLE", "{n}");
        assert_eq!(
            (cell(l, 0, 0), cell(l, 1, 0), cell(l, 0, 1)),
            ("Café".into(), "Zoë".into(), "1.5".into()),
            "{n}"
        );
    }
    // Delimiters, and European numbers in a file separated by semicolons.
    let l = ok("semicolon decimal comma");
    assert_eq!(
        (types(l).as_str(), cell(l, 0, 1), cell(l, 1, 1)),
        ("VARCHAR,DOUBLE", "1.5".into(), "2.25".into())
    );
    for n in ["pipe", "tab"] {
        assert_eq!(types(ok(n)), "VARCHAR,BIGINT", "{n}");
    }
    // Quoted values keep their line breaks and quotes.
    let l = ok("quoted newline");
    assert_eq!(
        (cell(l, 0, 1), cell(l, 1, 1)),
        ("line one\nline two".into(), "say \"hi\"".into())
    );
    // Short rows get blanks, a long row an extra column; nothing shifts.
    let l = ok("ragged");
    assert_eq!(names(l), "A,B,C,column3");
    assert_eq!(l.rows[1], ["4", "5", "", ""]);
    assert_eq!(l.rows[2], ["6", "7", "8", "9"]);
    assert_eq!(l.rows[3], ["10", "11", "12", ""]);
    // Headers are made unique and named when blank.
    assert_eq!(
        names(ok("dup and blank headers")),
        "Name,Name_1,column2,Amount"
    );
    assert!(ok("header only").rows.is_empty());
    assert!(ok("empty").rows.is_empty());
    assert_eq!(
        ok("crlf trailing delim").rows,
        [["1", "2", ""], ["3", "4", ""]]
    );
    // Thousands separators stay text until Change type (which reads them).
    assert_eq!(cell(ok("thousands"), 0, 1), "1,234.50");
    // Day-first and month-first dates are told apart by the values.
    assert_eq!(
        (cell(ok("dmy dates"), 0, 0), cell(ok("dmy dates"), 2, 0)),
        ("2024-01-13".into(), "2024-02-01".into())
    );
    assert_eq!(cell(ok("mdy dates"), 0, 0), "2024-01-13");
    assert_eq!(ok("500 columns").columns.len(), 500);
    // A value that doesn't fit the type worked out from the start of the file (here
    // after 30,000 numbers) doesn't fail the load: the file is read again for types.
    assert_eq!(
        ok("mixed types late").loaded,
        [
            charted_model::DataType::Int64,
            charted_model::DataType::String
        ]
    );
    // Title lines above the header are skipped.
    assert_eq!(names(ok("title rows")), "Name,Amount");
    // JSON: nested records and lists come as records and lists; uneven lines as blanks.
    assert!(types(ok("json nested")).contains("STRUCT"));
    assert_eq!(names(ok("ndjson uneven")), "a,b");
    assert!(err("json bad").contains("isn't valid JSON"));
    // Workbooks.
    let l = ok("xlsx Data");
    assert_eq!(types(l), "VARCHAR,DOUBLE,DATE,BIGINT");
    assert_eq!(cell(l, 0, 2), "2023-03-16");
    // A formula error is a blank cell, so the column stays a number.
    assert_eq!(cell(l, 2, 3), "");
    assert!(ok("xlsx Empty").rows.is_empty());
    // A title block above a sheet's header comes in as rows, to remove with "Remove
    // top rows" and "Use first row as headers" (as in Power BI).
    assert_eq!(names(ok("xlsx Report")), "Quarterly report,column1,column2");
    assert_eq!(ok("xlsx Merged header").rows[0], ["Region", "2023", "2024"]);
    assert!(err("xlsx Missing").contains("\"Missing\""));
    assert!(err("xlsx not a workbook").contains("as a workbook"));
    // Parquet types, broken files and missing ones.
    let l = ok("parquet types");
    assert_eq!(
        types(l),
        "DECIMAL(18,4),TIMESTAMP WITH TIME ZONE,STRUCT(a INTEGER, b VARCHAR),VARCHAR,INTEGER[],VARCHAR"
    );
    assert_eq!(cell(l, 0, 5), "é");
    assert!(err("parquet broken").contains("isn't a Parquet file"));
    assert!(
        err("missing file").contains("doesn't exist"),
        "{}",
        err("missing file")
    );
}

/// Five million rows (about 190 MB): typed by reading the whole file, still quick.
/// `cargo test -p charted-core --release --test torture -- --ignored`
#[test]
#[ignore]
fn a_large_csv_loads_in_seconds() {
    let mut fx = Fx::new();
    let p = fx.dir.path().join("big.csv");
    {
        use std::io::Write;
        let mut w = std::io::BufWriter::new(std::fs::File::create(&p).unwrap());
        writeln!(w, "Id,Region,Amount,When").unwrap();
        for i in 0..5_000_000u64 {
            writeln!(
                w,
                "{i},Region {},{}.{:02},2024-{:02}-{:02}",
                i % 50,
                i % 9973,
                i % 100,
                i % 12 + 1,
                i % 28 + 1
            )
            .unwrap();
        }
    }
    let t = std::time::Instant::now();
    let l = fx.load(csv(&p)).unwrap();
    let secs = t.elapsed().as_secs_f64();
    println!("5M rows in {secs:.1} s");
    assert_eq!(
        l.columns.iter().map(|c| c.1.as_str()).collect::<Vec<_>>(),
        ["BIGINT", "VARCHAR", "DOUBLE", "DATE"]
    );
    assert!(secs < 30.0, "{secs} s");
}

/// The query editor's first steps for a text file (first row as headers, then the
/// types its values read as) on numbers written "1.234,5", and on decimals that DuckDB
/// would round to whole numbers.
#[test]
fn types_are_detected_as_written() {
    use charted_core::queryedit::Analysis;
    let mut fx = Fx::new();
    let p = fx.file(
        "eu.csv",
        latin1("Produit;Prix;Qté\nCafé crème;2,50;3\nThé glacé;1.234,75;12\n"),
    );
    let detect = |fx: &Fx, q: &Query| -> Vec<(String, charted_model::DataType)> {
        serde_json::from_slice(
            &fx.app
                .data(DataRequest::QueryAnalyze {
                    query: q.clone(),
                    queries: vec![],
                    upto: q.steps.len(),
                    analysis: Analysis::DetectTypes,
                })
                .unwrap(),
        )
        .unwrap()
    };
    let mut q = Query {
        id: "q".into(),
        name: "Prices".into(),
        table: "q".into(),
        source: Source::Csv {
            path: p.display().to_string(),
            delimiter: None,
            header: false,
            encoding: None,
            decimal: None,
        },
        steps: vec![Step::PromoteHeaders {
            names: vec!["Produit".into(), "Prix".into(), "Qté".into()],
        }],
        step_names: BTreeMap::new(),
        no_load: false,
    };
    use charted_model::DataType as T;
    let types = detect(&fx, &q);
    assert_eq!(
        types,
        [
            ("Produit".into(), T::String),
            ("Prix".into(), T::Double),
            ("Qté".into(), T::Int64)
        ]
    );
    q.steps.push(Step::ChangeTypes { columns: types });
    let rows = rows(
        fx.app
            .data(DataRequest::QueryPreview {
                query: q.clone(),
                queries: vec![],
                upto: 2,
                limit: 10,
            })
            .unwrap(),
    );
    assert_eq!(
        rows,
        [["Café crème", "2.5", "3"], ["Thé glacé", "1234.75", "12"]]
    );
    // Set on the source, the decimal comma goes to Power BI as the culture to read
    // the types in.
    if let Source::Csv { decimal, .. } = &mut q.source {
        *decimal = Some(",".into());
    }
    assert!(to_m(&q, &[]).contains(", \"de-DE\")"), "{}", to_m(&q, &[]));

    // Decimals in a file written with points aren't whole numbers.
    let p = fx.file("pt.csv", "Item,Price\nA,2.5\nB,10\n");
    let q = Query {
        source: Source::Csv {
            path: p.display().to_string(),
            delimiter: None,
            header: false,
            encoding: None,
            decimal: None,
        },
        steps: vec![Step::PromoteHeaders {
            names: vec!["Item".into(), "Price".into()],
        }],
        ..q
    };
    assert_eq!(detect(&fx, &q)[1], ("Price".into(), T::Double));
    let _ = fx.load(csv(&p)).unwrap();
}
