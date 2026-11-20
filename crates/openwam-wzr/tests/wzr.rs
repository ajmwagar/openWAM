//! Reader/writer contract tests. Real WAM samples are never committed; set
//! `OPENWAM_WZR_SAMPLES=<dir>` to also run every `*.wzr` there.

use openwam_wzr::{WzrError, bbox::wam_file_bb, read, write};
use std::path::PathBuf;

const FIXTURE: &[u8] = include_bytes!("fixtures/two-instances.wzr");

#[test]
fn authored_fixture_is_byte_identical() {
    let p = read(FIXTURE).expect("fixture reads");
    assert_eq!(p.file_list.files[0].group_instances, vec![vec![0, 1]]);
    let once = write(&p).unwrap();
    assert_eq!(once, FIXTURE, "read -> write must reproduce the file");
    let twice = write(&read(&once).unwrap()).unwrap();
    assert_eq!(twice, once, "write -> read -> write must be stable");
    assert!(!once.ends_with(b"\n") && !once.starts_with(&[0xEF]));
}

#[test]
fn authored_fixture_file_bb_matches_svg() {
    let p = read(FIXTURE).unwrap();
    let f = &p.file_list.files[0];
    let r = wam_file_bb(&f.svg_string).unwrap();
    assert_eq!(
        [Some(r.x0), Some(r.y0), Some(r.x1), Some(r.y1)],
        [
            f.file_bb.min_x,
            f.file_bb.min_y,
            f.file_bb.max_x,
            f.file_bb.max_y
        ]
    );
}

fn text() -> String {
    String::from_utf8(FIXTURE.to_vec()).unwrap()
}

#[test]
fn rejects_bom() {
    let mut b = vec![0xEF, 0xBB, 0xBF];
    b.extend_from_slice(FIXTURE);
    assert!(matches!(read(&b), Err(WzrError::Bom)));
}

#[test]
fn rejects_unparseable_and_missing_keys() {
    assert!(matches!(read(b"{\"fileList\":"), Err(WzrError::Schema(_))));
    let t = text().replacen("\"cutQuality\":\"mediumRate\",", "", 1);
    assert!(matches!(read(t.as_bytes()), Err(WzrError::Schema(_))));
}

#[test]
fn rejects_unknown_keys_and_enum_values() {
    let t = text().replacen("{\"wamVersion\"", "{\"extra\":1,\"wamVersion\"", 1);
    assert!(matches!(read(t.as_bytes()), Err(WzrError::Schema(_))));
    let t = text().replacen("\"type\":\"Outside\"", "\"type\":\"Pocket\"", 1);
    assert!(matches!(read(t.as_bytes()), Err(WzrError::Schema(_))));
    let t = text().replacen(
        "\"abrasiveMode\":\"speedMode\"",
        "\"abrasiveMode\":\"turbo\"",
        1,
    );
    assert!(matches!(read(t.as_bytes()), Err(WzrError::Schema(_))));
    let t = text().replacen("\"segments\":[null]", "\"segments\":[{\"a\":1}]", 1);
    assert!(matches!(read(t.as_bytes()), Err(WzrError::Schema(_))));
}

#[test]
fn rejects_inconsistent_groups() {
    let t = text().replacen(
        "\"groupInstances\":[[0,1]]",
        "\"groupInstances\":[[0,2]]",
        1,
    );
    assert!(matches!(read(t.as_bytes()), Err(WzrError::Inconsistent(_))));
}

#[test]
fn rejects_non_finite_on_write() {
    let mut p = read(FIXTURE).unwrap();
    p.placements[0].instances[0].position.x = f64::NAN;
    assert!(matches!(write(&p), Err(WzrError::NonFinite)));
}

#[test]
fn real_samples_round_trip_when_present() {
    let Some(dir) = std::env::var_os("OPENWAM_WZR_SAMPLES").map(PathBuf::from) else {
        eprintln!("OPENWAM_WZR_SAMPLES unset; skipping real samples");
        return;
    };
    let mut n = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wzr") {
            continue;
        }
        let bytes = std::fs::read(&path).unwrap();
        let p = read(&bytes).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
        assert_eq!(
            write(&p).unwrap(),
            bytes,
            "{} not byte-identical",
            path.display()
        );
        if p.wam_version.is_some() {
            // WAM-written: our fileBB rule reproduces WAM's numbers.
            for f in &p.file_list.files {
                let r = wam_file_bb(&f.svg_string).unwrap();
                for (got, want) in [r.x0, r.y0, r.x1, r.y1].into_iter().zip([
                    f.file_bb.min_x,
                    f.file_bb.min_y,
                    f.file_bb.max_x,
                    f.file_bb.max_y,
                ]) {
                    assert!(
                        (got - want.unwrap()).abs() < 1e-9,
                        "{} {}: {got} vs {want:?}",
                        path.display(),
                        f.filename
                    );
                }
            }
        }
        n += 1;
    }
    assert!(n > 0, "no .wzr in {}", dir.display());
}
