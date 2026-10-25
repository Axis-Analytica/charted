/// A new random identifier for model objects (a UUID, like Power BI lineage tags).
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// A deterministic identifier derived from a path such as "column/Sales/Amount", so the
/// sample project is byte-identical every time it is generated.
pub fn stable_id(path: &str) -> String {
    const NAMESPACE: uuid::Uuid = uuid::uuid!("6f1d3c2a-8b8e-4f3c-9a51-2d6f0b7c9e10");
    uuid::Uuid::new_v5(&NAMESPACE, path.as_bytes()).to_string()
}
