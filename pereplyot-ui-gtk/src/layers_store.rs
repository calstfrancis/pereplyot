//! Other people's notes on a document (`layers/<hash>/<name>.json`), kept apart from your own sidecar.

use std::path::PathBuf;

use fond_read_gtk::fsutil;
use fond_read_gtk::layer::LayerFile;

fn dir(hash: &str) -> PathBuf {
    crate::paths::data_dir().join("layers").join(hash)
}

pub fn load(hash: &str) -> Vec<LayerFile> {
    let Ok(entries) = std::fs::read_dir(dir(hash)) else {
        return Vec::new();
    };
    let mut out: Vec<LayerFile> = entries
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == "json"))
        .filter_map(|e| LayerFile::parse(&std::fs::read_to_string(e.path()).ok()?).ok())
        .collect();
    out.sort_by_key(|l| l.name.to_lowercase());
    out
}

pub fn save(hash: &str, layer: &LayerFile) -> Result<(), String> {
    let json = layer.to_json()?;
    fsutil::write_atomic(
        &dir(hash).join(format!("{}.json", layer.stem())),
        json.as_bytes(),
    )
    .map_err(|e| e.to_string())
}

pub fn remove(hash: &str, name: &str) {
    for layer in load(hash).into_iter().filter(|l| l.name == name) {
        let _ = std::fs::remove_file(dir(hash).join(format!("{}.json", layer.stem())));
    }
}
