use tempfile::tempdir;
use xchendev_tools_lib::config_store::open_at;

#[test]
fn export_import_roundtrip() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("config.json");
    let store = open_at(path);
    store
        .save_manual_override("node", "C:\\node\\node.exe")
        .unwrap();
    let exported = store.export_json().unwrap();
    let store2 = open_at(dir.path().join("config2.json"));
    store2.import_json(&exported).unwrap();
    assert_eq!(
        store2.get_manual_override("node"),
        Some("C:\\node\\node.exe".to_string())
    );
}
