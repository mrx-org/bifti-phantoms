//! `reslice_to.move_to`: placing the resampled grid independently of the sampling.

use std::path::PathBuf;

use bifti::{BiftiPhantom, DEFAULT_MAX_SUBSTEPS, MoveTo, Phantom, ResliceTo};

const SAMPLED: [[f64; 4]; 3] = [
    [3.0, 0.0, 0.0, -50.0],
    [0.0, 2.0, 0.0, 7.0],
    [0.0, 0.0, 5.0, 12.0],
];

fn data_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../python/bifti/examples/data")
}

fn position(affine: [[f64; 4]; 3], voxel: [usize; 3]) -> [f64; 3] {
    affine.map(|row| (0..3).map(|j| row[j] * voxel[j] as f64).sum::<f64>() + row[3])
}

fn assert_close(a: [[f64; 4]; 3], b: [[f64; 4]; 3], tol: f64) {
    for (ra, rb) in a.iter().zip(&b) {
        for (x, y) in ra.iter().zip(rb) {
            assert!((x - y).abs() <= tol, "{a:?} != {b:?}");
        }
    }
}

#[test]
fn without_move_to_the_grid_stays_where_it_was_sampled() {
    let r = ResliceTo::new(SAMPLED, [4, 5, 1]);
    assert_eq!(r.placed_affine(), SAMPLED);
    let json = serde_json::to_value(r).unwrap();
    assert!(json.get("move_to").is_none());
}

#[test]
fn center_puts_voxel_n_div_2_at_the_origin() {
    for n in [4, 5] {
        let r = ResliceTo {
            move_to: Some(MoveTo::Center),
            ..ResliceTo::new(SAMPLED, [n, n, 1])
        };
        let placed = r.placed_affine();
        for i in 0..3 {
            assert_eq!(placed[i][..3], SAMPLED[i][..3]);
        }
        // even: -n/2 .. n/2-1, odd: -(n-1)/2 .. (n-1)/2
        for i in 0..n {
            let x = position(placed, [i, 0, 0])[0] / 3.0;
            assert_eq!(x, i as f64 - (n / 2) as f64, "n = {n}");
        }
    }
}

#[test]
fn center_keeps_rotation() {
    let (c, s) = (0.8, 0.6);
    let affine = [
        [2.0 * c, -2.0 * s, 0.0, 10.0],
        [2.0 * s, 2.0 * c, 0.0, -4.0],
        [0.0, 0.0, 3.0, 1.0],
    ];
    let r = ResliceTo {
        move_to: Some(MoveTo::Center),
        ..ResliceTo::new(affine, [6, 4, 2])
    };
    for x in position(r.placed_affine(), [3, 2, 1]) {
        assert!(x.abs() < 1e-12);
    }
}

#[test]
fn move_to_round_trips_through_json() {
    let target = [
        [3.0, 0.0, 0.0, 1.0],
        [0.0, 2.0, 0.0, 2.0],
        [0.0, 0.0, 5.0, 3.0],
    ];
    for move_to in [MoveTo::Center, MoveTo::Affine(target)] {
        let r = ResliceTo {
            move_to: Some(move_to),
            ..ResliceTo::new(SAMPLED, [4, 5, 1])
        };
        let back: ResliceTo = serde_json::from_value(serde_json::to_value(r).unwrap()).unwrap();
        assert_eq!(back, r);
    }
    let r: ResliceTo = serde_json::from_value(serde_json::json!({
        "affine": SAMPLED, "resolution": [4, 5, 1], "move_to": target,
    }))
    .unwrap();
    assert_eq!(r.placed_affine(), target);
}

#[test]
fn invalid_move_to_is_rejected() {
    for move_to in [
        serde_json::json!("middle"),
        serde_json::json!([[1, 0, 0], [0, 1, 0]]),
    ] {
        let result: Result<ResliceTo, _> = serde_json::from_value(serde_json::json!({
            "affine": SAMPLED, "resolution": [4, 5, 1], "move_to": move_to,
        }));
        assert!(result.is_err(), "{move_to} was accepted");
    }
}

#[test]
fn move_to_changes_placement_not_sampling() {
    let config = BiftiPhantom::load(data_dir().join("shapes_resliced.json")).unwrap();
    let plain =
        Phantom::load_from_config(config.clone(), data_dir(), DEFAULT_MAX_SUBSTEPS).unwrap();

    let mut moved_config = config.clone();
    moved_config.reslice_to.as_mut().unwrap().move_to = Some(MoveTo::Center);
    let placed = moved_config.reslice_to.unwrap().placed_affine();
    let moved = Phantom::load_from_config(moved_config, data_dir(), DEFAULT_MAX_SUBSTEPS).unwrap();

    for (name, tissue) in &moved.tissues {
        let reference = &plain.tissues[name];
        assert_eq!(
            tissue.density.data.to_f64_vec(),
            reference.density.data.to_f64_vec(),
            "{name}"
        );
        assert_eq!(
            tissue.t1.data.to_f64_vec(),
            reference.t1.data.to_f64_vec(),
            "{name}"
        );
        for volume in [&tissue.density, &tissue.t1, &tissue.db0] {
            assert_eq!(volume.affine, placed, "{name}");
        }
        for volume in tissue.b1_tx.iter().chain(&tissue.b1_rx) {
            assert_eq!(volume.affine, placed, "{name}");
        }
    }
}

#[test]
fn saved_phantom_keeps_the_placed_affine() {
    let mut config = BiftiPhantom::load(data_dir().join("shapes_resliced.json")).unwrap();
    config.reslice_to.as_mut().unwrap().move_to = Some(MoveTo::Center);
    let placed = config.reslice_to.unwrap().placed_affine();

    let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("move_to");
    let _ = std::fs::remove_dir_all(&dir);
    let json = Phantom::load_from_config(config, data_dir(), DEFAULT_MAX_SUBSTEPS)
        .unwrap()
        .save(&dir, "phantom.json", None)
        .unwrap();

    let reloaded = Phantom::load(json, DEFAULT_MAX_SUBSTEPS).unwrap();
    assert!(reloaded.config.reslice_to.is_none());
    for tissue in reloaded.tissues.values() {
        assert_close(tissue.density.affine, placed, 1e-4);
    }
}
