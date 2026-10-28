//! Saves loaded phantoms and checks the directory reloads to the same values, is
//! byte-for-byte reproducible, and shares bit-identical volumes.

use std::path::{Path, PathBuf};

use bifti::{BiftiPhantom, DEFAULT_MAX_SUBSTEPS, Phantom, TissueProperty, Volume, VolumeData};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../python/bifti/examples/data")
}

fn out_dir(name: &str) -> PathBuf {
    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR"))
        .join("phantom_save")
        .join(name);
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

fn as_f64(data: &VolumeData) -> Vec<f64> {
    data.to_f64_vec()
}

fn assert_same_volume(a: &Volume, b: &Volume, what: &str) {
    // A constant stays a constant in the saved phantom, so it reloads as a single voxel
    // where the original (resliced) one was expanded onto the grid: compare per voxel.
    let n = |v: &Volume| v.shape.iter().product::<usize>();
    if n(a) > 1 && n(b) > 1 {
        assert_eq!(a.shape, b.shape, "{what} shape");
        for (x, y) in a.affine.iter().flatten().zip(b.affine.iter().flatten()) {
            assert!((x - y).abs() < 1e-5, "{what} affine {x} vs {y}");
        }
    }
    let (da, db) = (as_f64(&a.data), as_f64(&b.data));
    for i in 0..n(a).max(n(b)) {
        let (x, y) = (da[i % da.len()], db[i % db.len()]);
        assert_eq!(x, y, "{what} voxel {i}");
    }
}

fn assert_same_phantom(a: &Phantom, b: &Phantom) {
    assert_eq!(a.tissues.len(), b.tissues.len());
    for (name, ta) in &a.tissues {
        let tb = &b.tissues[name];
        for (what, va, vb) in [
            ("density", &ta.density, &tb.density),
            ("T1", &ta.t1, &tb.t1),
            ("T2", &ta.t2, &tb.t2),
            ("T2'", &ta.t2dash, &tb.t2dash),
            ("ADC", &ta.adc, &tb.adc),
            ("dB0", &ta.db0, &tb.db0),
        ] {
            assert_same_volume(va, vb, &format!("{name} {what}"));
        }
        assert_eq!(ta.b1_tx.len(), tb.b1_tx.len(), "{name} B1+ channels");
        for (k, (va, vb)) in ta.b1_tx.iter().zip(&tb.b1_tx).enumerate() {
            assert_same_volume(va, vb, &format!("{name} B1+[{k}]"));
        }
        for (k, (va, vb)) in ta.b1_rx.iter().zip(&tb.b1_rx).enumerate() {
            assert_same_volume(va, vb, &format!("{name} B1-[{k}]"));
        }
    }
}

fn files_in(dir: &Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn round_trips_a_native_phantom_with_a_mapping() {
    let original = Phantom::load(data_dir().join("shapes.json"), DEFAULT_MAX_SUBSTEPS).unwrap();
    let dir = out_dir("native");
    let json = original.save(&dir, "phantom.json").unwrap();
    assert_eq!(json, dir.join("phantom.json"));

    let saved = BiftiPhantom::load(&json).unwrap();
    assert!(saved.reslice_to.is_none());
    // `func` is baked in: no tissue keeps a mapping
    for tissue in saved.tissues.values() {
        assert!(!matches!(tissue.properties.db0, TissueProperty::Mapping(_)));
    }
    // constants stay constants
    assert!(matches!(saved.tissues["disk"].properties.t1, TissueProperty::Value(v) if v == 1.0));

    assert_same_phantom(
        &original,
        &Phantom::load(&json, DEFAULT_MAX_SUBSTEPS).unwrap(),
    );
}

#[test]
fn resliced_save_is_native_on_the_target_grid() {
    let original = Phantom::load(
        data_dir().join("shapes_resliced.json"),
        DEFAULT_MAX_SUBSTEPS,
    )
    .unwrap();
    let dir = out_dir("resliced");
    let json = original.save(&dir, "phantom.json").unwrap();

    let saved = BiftiPhantom::load(&json).unwrap();
    assert!(saved.reslice_to.is_none());
    let reloaded = Phantom::load(&json, DEFAULT_MAX_SUBSTEPS).unwrap();
    for tissue in reloaded.tissues.values() {
        assert_eq!(tissue.density.shape, [60, 48, 4]);
    }
    assert_same_phantom(&original, &reloaded);
}

#[test]
fn round_trips_channels_and_a_mapping_of_the_subj42_phantom() {
    let original = Phantom::load(data_dir().join("subj42-3T.json"), DEFAULT_MAX_SUBSTEPS).unwrap();
    let dir = out_dir("subj42");
    let json = original.save(&dir, "phantom.json").unwrap();
    assert_same_phantom(
        &original,
        &Phantom::load(&json, DEFAULT_MAX_SUBSTEPS).unwrap(),
    );
}

#[test]
fn output_is_deterministic() {
    let phantom = Phantom::load(
        data_dir().join("shapes_resliced.json"),
        DEFAULT_MAX_SUBSTEPS,
    )
    .unwrap();
    let (a, b) = (out_dir("det_a"), out_dir("det_b"));
    phantom.save(&a, "phantom.json").unwrap();
    // load again so the HashMap iteration order may differ
    Phantom::load(
        data_dir().join("shapes_resliced.json"),
        DEFAULT_MAX_SUBSTEPS,
    )
    .unwrap()
    .save(&b, "phantom.json")
    .unwrap();
    assert_eq!(files_in(&a), files_in(&b));

    // gzip header: no mtime, no name flag, OS unknown
    let gz = std::fs::read(a.join("density.nii.gz")).unwrap();
    assert_eq!(&gz[..2], &[0x1f, 0x8b]);
    assert_eq!(gz[3], 0, "flags");
    assert_eq!(&gz[4..8], &[0; 4], "mtime");
    assert_eq!(gz[9], 255, "os");
}

#[test]
fn identical_maps_share_one_volume() {
    // Without reslice_to, two tissues referencing the same map and func are bit-identical.
    let mut config = BiftiPhantom::load(data_dir().join("shapes.json")).unwrap();
    let ring = config.tissues["ring"].clone();
    config.tissues.insert("ring2".into(), ring);
    let phantom = Phantom::load_from_config(config, data_dir(), DEFAULT_MAX_SUBSTEPS).unwrap();

    let dir = out_dir("dedup");
    let json = phantom.save(&dir, "phantom.json").unwrap();
    let saved = BiftiPhantom::load(&json).unwrap();
    let (a, b) = (&saved.tissues["ring"], &saved.tissues["ring2"]);
    let (TissueProperty::Ref(ra), TissueProperty::Ref(rb)) = (&a.properties.db0, &b.properties.db0)
    else {
        panic!("expected plain refs")
    };
    assert_eq!(ra, rb);
    assert_eq!(a.density, b.density);
}
