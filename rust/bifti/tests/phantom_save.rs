//! Saves loaded phantoms and checks the directory reloads to the same values, is
//! byte-for-byte reproducible, and shares bit-identical volumes.

use std::path::{Path, PathBuf};

use bifti::{
    BiftiPhantom, DEFAULT_MAX_SUBSTEPS, NiftiMapping, NiftiRef, Phantom, TissueProperty, Volume,
    VolumeData,
};
use num_complex::Complex;

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

/// The values as complex numbers, so complex maps compare in full rather than by magnitude.
fn values(data: &VolumeData) -> Vec<Complex<f64>> {
    match data {
        VolumeData::Complex64(items) => items
            .iter()
            .map(|x| Complex::new(x.re as f64, x.im as f64))
            .collect(),
        VolumeData::Complex128(items) => items.clone(),
        real => real.to_f64_vec().into_iter().map(Complex::from).collect(),
    }
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
    let (da, db) = (values(&a.data), values(&b.data));
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
    let json = original.save(&dir, "phantom.json", None).unwrap();
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
    let json = original.save(&dir, "phantom.json", None).unwrap();

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
    let json = original.save(&dir, "phantom.json", None).unwrap();
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
    phantom.save(&a, "phantom.json", None).unwrap();
    // load again so the HashMap iteration order may differ
    Phantom::load(
        data_dir().join("shapes_resliced.json"),
        DEFAULT_MAX_SUBSTEPS,
    )
    .unwrap()
    .save(&b, "phantom.json", None)
    .unwrap();
    assert_eq!(files_in(&a), files_in(&b));

    // gzip header: no mtime, no name flag, OS unknown
    let gz = std::fs::read(a.join("phantom_density.nii.gz")).unwrap();
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
    let json = phantom.save(&dir, "phantom.json", None).unwrap();
    let saved = BiftiPhantom::load(&json).unwrap();
    let (a, b) = (&saved.tissues["ring"], &saved.tissues["ring2"]);
    let (TissueProperty::Ref(ra), TissueProperty::Ref(rb)) = (&a.properties.db0, &b.properties.db0)
    else {
        panic!("expected plain refs")
    };
    assert_eq!(ra, rb);
    assert_eq!(a.density, b.density);
}

#[test]
fn file_names_are_prefixed_with_the_phantom_name() {
    let dir = out_dir("prefix");
    for (name, json_name) in [("shapes.json", "a.json"), ("subj42-3T.json", "b.json")] {
        let original = Phantom::load(data_dir().join(name), DEFAULT_MAX_SUBSTEPS).unwrap();
        original.save(&dir, json_name, None).unwrap();
    }
    for (name, json_name, prefix) in [
        ("shapes.json", "a.json", "a_"),
        ("subj42-3T.json", "b.json", "b_"),
    ] {
        let json = dir.join(json_name);
        for tissue in BiftiPhantom::load(&json).unwrap().tissues.values() {
            assert!(
                tissue
                    .density
                    .file_name
                    .to_string_lossy()
                    .starts_with(prefix)
            );
        }
        assert_same_phantom(
            &Phantom::load(data_dir().join(name), DEFAULT_MAX_SUBSTEPS).unwrap(),
            &Phantom::load(&json, DEFAULT_MAX_SUBSTEPS).unwrap(),
        );
    }
}

#[test]
fn constant_threshold_stores_near_constant_maps_as_constants() {
    let mut phantom = Phantom::load(data_dir().join("shapes.json"), DEFAULT_MAX_SUBSTEPS).unwrap();
    let disk = phantom.tissues.get_mut("disk").unwrap();
    let n = disk.db0.data.len();
    let mut db0 = vec![3.0; n];
    db0[0] += 1e-3;
    let mean = db0.iter().sum::<f64>() / n as f64;
    let std = (db0.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n as f64).sqrt();
    disk.db0.data = VolumeData::Float64(db0);
    disk.b1_tx[1].data = VolumeData::Float32(vec![1.0; n]);

    let save = |name: &str, threshold| {
        let json = phantom
            .save(out_dir(name), "phantom.json", threshold)
            .unwrap();
        BiftiPhantom::load(json).unwrap().tissues["disk"].clone()
    };
    let is_ref = |p: &TissueProperty| matches!(p, TissueProperty::Ref(_));
    let value = |p: &TissueProperty| match p {
        TissueProperty::Value(v) => *v,
        _ => panic!("expected a constant"),
    };

    let off = save("const_off", None);
    assert!(is_ref(&off.properties.db0) && is_ref(&off.properties.b1_tx[1]));

    let zero = save("const_zero", Some(0.0));
    assert!(is_ref(&zero.properties.db0));
    assert_eq!(value(&zero.properties.b1_tx[1]), 1.0);

    let on = save("const_on", Some(2.0 * std));
    assert!((value(&on.properties.db0) - mean).abs() < 1e-12);
    assert_eq!(value(&on.properties.b1_tx[1]), 1.0);
    assert!(!out_dir("const_on").join("phantom_B1plus_1.nii.gz").exists());

    let negative = save("const_negative", Some(-1.0));
    assert!(is_ref(&negative.properties.b1_tx[1]));
}

#[test]
fn complex_maps_round_trip() {
    for name in ["shapes.json", "shapes_resliced.json"] {
        let native = Phantom::load(data_dir().join("shapes.json"), DEFAULT_MAX_SUBSTEPS).unwrap();
        let grid = &native.tissues["disk"].density;
        let channel = |k: f32| {
            let [nx, ny, nz] = grid.shape;
            let data = (0..nx * ny * nz)
                .map(|i| Complex::from_polar(1.0f32, k * (i % nx) as f32 / nx as f32 * 3.0))
                .collect();
            Volume {
                affine: grid.affine,
                shape: grid.shape,
                data: VolumeData::Complex64(data),
            }
        };
        let dir = out_dir(&format!("complex_{name}"));
        let b1 = dir.join("b1.nii.gz");
        std::fs::create_dir_all(&dir).unwrap();
        Volume::save_nifti_stack(&[&channel(1.0), &channel(-2.0)], &b1).unwrap();

        let mut config = BiftiPhantom::load(data_dir().join(name)).unwrap();
        let reference = |tissue_index| NiftiRef {
            file_name: b1.clone(),
            tissue_index,
        };
        let disk = &mut config.tissues.get_mut("disk").unwrap().properties;
        disk.b1_tx = vec![
            TissueProperty::Ref(reference(0)),
            TissueProperty::Value(1.0),
        ];
        disk.b1_rx = vec![TissueProperty::Mapping(NiftiMapping {
            file: reference(1),
            func: "x * 2".into(),
        })];
        let phantom = Phantom::load_from_config(config, data_dir(), DEFAULT_MAX_SUBSTEPS).unwrap();
        let loaded = &phantom.tissues["disk"];
        assert!(matches!(loaded.b1_tx[0].data, VolumeData::Complex64(_)));
        assert!(
            values(&loaded.b1_tx[0].data)
                .iter()
                .any(|x| x.im.abs() > 0.5)
        );

        let json = phantom.save(dir.join("out"), "phantom.json", None).unwrap();
        assert_same_phantom(
            &phantom,
            &Phantom::load(&json, DEFAULT_MAX_SUBSTEPS).unwrap(),
        );
    }
}
