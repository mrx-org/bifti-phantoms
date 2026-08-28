//! Loads the example phantoms in `python/bifti/examples/data/`, exercising the whole
//! load path: the NIfTI cache, density-weighted resampling and `func` mapping evaluation.

use std::path::PathBuf;

use bifti::{Phantom, VolumeData};

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../python/bifti/examples/data")
}

fn as_f64(data: &VolumeData) -> Vec<f64> {
    use bifti::VolumeDataElement as E;
    match data {
        VolumeData::Float32(v) => v.iter().map(|&x| E::to_f64(x)).collect(),
        VolumeData::Float64(v) => v.clone(),
        other => panic!("unexpected data type in fixture: {}", other.len()),
    }
}

#[test]
fn loads_the_native_shapes_phantom() {
    let phantom = Phantom::load(data_dir().join("shapes.json")).expect("load shapes.json");
    assert!(!phantom.tissues.is_empty());
    for tissue in phantom.tissues.values() {
        assert_eq!(tissue.density.shape, [40, 32, 4]);
        assert!(!tissue.b1_tx.is_empty());
    }
}

#[test]
fn loads_the_resliced_shapes_phantom_onto_the_target_grid() {
    let phantom =
        Phantom::load(data_dir().join("shapes_resliced.json")).expect("load shapes_resliced.json");
    for (name, tissue) in &phantom.tissues {
        assert_eq!(tissue.density.shape, [60, 48, 4], "{name} density");
        assert_eq!(tissue.t1.shape, [60, 48, 4], "{name} T1");
        for ch in &tissue.b1_tx {
            assert_eq!(ch.shape, [60, 48, 4], "{name} B1+");
        }
        assert!(
            as_f64(&tissue.density.data).iter().all(|v| v.is_finite()),
            "{name} density has non-finite values"
        );
    }
}

/// The property that motivates density weighting: resampling must not invent T1 values
/// outside the range present in the source. Plain averaging against background zeros does
/// exactly that at every tissue and FOV edge.
#[test]
fn resampling_keeps_t1_within_the_source_range() {
    let native = Phantom::load(data_dir().join("shapes.json")).expect("load shapes.json");
    let resliced =
        Phantom::load(data_dir().join("shapes_resliced.json")).expect("load shapes_resliced.json");

    for (name, tissue) in &resliced.tissues {
        let src = as_f64(&native.tissues[name].t1.data);
        let lo = src.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = src.iter().copied().fold(f64::NEG_INFINITY, f64::max);

        for &v in as_f64(&tissue.t1.data).iter() {
            assert!(
                v >= lo - 1e-6 && v <= hi + 1e-6,
                "{name}: resampled T1 {v} outside source range [{lo}, {hi}]"
            );
        }
    }
}

/// The 100x100 single-slice phantom also carries 8 `B1+` channels and a `func` mapping.
#[test]
fn loads_the_subj42_phantom_with_channels_and_a_mapping() {
    let phantom = Phantom::load(data_dir().join("subj42-3T.json")).expect("load subj42-3T.json");
    for (name, tissue) in &phantom.tissues {
        assert_eq!(tissue.density.shape, [100, 100, 1], "{name}");
        // Channel counts differ per tissue (only `gm` is multi-channel here); every
        // channel must land on the target grid regardless.
        assert!(!tissue.b1_tx.is_empty(), "{name} has no B1+ channel");
        for ch in &tissue.b1_tx {
            assert_eq!(ch.shape, [100, 100, 1], "{name} B1+ channel");
        }
        assert!(as_f64(&tissue.db0.data).iter().all(|v| v.is_finite()));
    }
    assert_eq!(phantom.tissues["gm"].b1_tx.len(), 8, "gm B1+ channels");
}
