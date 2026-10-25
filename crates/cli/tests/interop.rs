//! `charted export` and `charted import`: an Ossie round trip changes nothing, and a
//! PBIP export writes a TMDL model.

use std::process::Command;

fn charted(args: &[&str]) -> (bool, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_charted-headless"))
        .args(args)
        .output()
        .unwrap();
    (
        out.status.success(),
        format!(
            "{}{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        ),
    )
}

#[test]
fn ossie_round_trip_and_pbip_export() {
    let dir = tempfile::tempdir().unwrap();
    let project = dir.path().join("p");
    let p = project.display().to_string();
    let (ok, out) = charted(&["sample", "create", &p]);
    assert!(ok, "{out}");

    let ossie = dir.path().join("model.ossie.yaml").display().to_string();
    let (ok, out) = charted(&["export", "ossie", &p, &ossie]);
    assert!(ok, "{out}");
    let (ok, out) = charted(&["import", &p, &ossie]);
    assert!(ok, "{out}");
    assert!(out.contains("0 differences"), "{out}");

    // An edited document reports what it changes, and importing applies it.
    let text = std::fs::read_to_string(&ossie)
        .unwrap()
        .replace("name: Total Sales", "name: Revenue");
    std::fs::write(&ossie, text).unwrap();
    let (ok, out) = charted(&["import", "--dry-run", &p, &ossie]);
    assert!(ok, "{out}");
    assert!(out.contains("measure Total Sales — name changed"), "{out}");
    let (ok, out) = charted(&["import", &p, &ossie]);
    assert!(ok, "{out}");
    let (_, out) = charted(&["import", "--dry-run", &p, &ossie]);
    assert!(out.contains("0 differences"), "{out}");

    let pbip = dir.path().join("pbip").display().to_string();
    let (ok, out) = charted(&["export", "pbip", &p, &pbip]);
    assert!(ok, "{out}");
    let sales = std::fs::read_to_string(
        dir.path()
            .join("pbip/Harbor & Pine.SemanticModel/definition/tables/Sales.tmdl"),
    )
    .unwrap();
    assert!(sales.contains("measure Revenue ="), "{sales}");
    assert!(sales.contains("partition Sales = m"), "{sales}");
    let rels = std::fs::read_to_string(
        dir.path()
            .join("pbip/Harbor & Pine.SemanticModel/definition/relationships.tmdl"),
    )
    .unwrap();
    assert!(
        rels.contains("isActive: false"),
        "the ship-date relationship is inactive:\n{rels}"
    );
}
