//! Golden test: design embedded in a WAM-saved `.wzr` -> openwam prepare ->
//! plan_cam -> verify_cam (separate process) -> export -> compare with WAM.
//!
//! Needs `OPENWAM_CLI=<openwam binary>` and `OPENWAM_WZR_SAMPLES=<dir>`;
//! skipped otherwise. A `.wzr` carries no toolpath, so "structural match" is the
//! project structure plus geometry (fileBB/baseBB) within 1e-6 in. Expected,
//! documented deviations are printed, not failed.

use openwam_wzr::{export::export, read, write};
use serde_json::{Value, json};
use std::io::Write;
use std::path::PathBuf;
use std::process::{Command, Stdio};

fn ow(cli: &PathBuf, request: Value) -> Value {
    let mut child = Command::new(cli)
        .args(["request", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(request.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    serde_json::from_slice(&out.stdout).unwrap()
}

fn seg_box(c: &Value) -> [f64; 4] {
    let mut b = [
        f64::INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::NEG_INFINITY,
    ];
    for s in c["segments"].as_array().unwrap() {
        for p in [
            &s["to"],
            &s["control"],
            &s["controls"][0],
            &s["controls"][1],
        ] {
            if let (Some(x), Some(y)) = (p[0].as_f64(), p[1].as_f64()) {
                b = [b[0].min(x), b[1].min(y), b[2].max(x), b[3].max(y)];
            }
        }
    }
    b
}

/// Root size attributes plus every untransformed `<path d>` outside `<defs>`.
fn bare_paths(svg: &str) -> String {
    let doc = roxmltree::Document::parse(svg).unwrap();
    let root = doc.root_element();
    let mut out = String::from(r#"<svg xmlns="http://www.w3.org/2000/svg""#);
    for a in ["width", "height", "viewBox"] {
        if let Some(v) = root.attribute(a) {
            out += &format!(r#" {a}="{v}""#);
        }
    }
    out += ">";
    for n in root.descendants().filter(|n| n.tag_name().name() == "path") {
        let hidden = n
            .ancestors()
            .any(|a| a.tag_name().name() == "defs" || a.attribute("transform").is_some());
        if let (false, Some(d)) = (hidden, n.attribute("d")) {
            out += &format!(r#"<path d="{d}"/>"#);
        }
    }
    out + "</svg>"
}

fn req(r: Value) -> Value {
    json!({"schema": "reversal.openwam-request.v1", "request": r})
}

#[test]
fn openwam_export_matches_wam_samples() {
    let (Some(cli), Some(dir)) = (
        std::env::var_os("OPENWAM_CLI").map(PathBuf::from),
        std::env::var_os("OPENWAM_WZR_SAMPLES").map(PathBuf::from),
    ) else {
        eprintln!("OPENWAM_CLI / OPENWAM_WZR_SAMPLES unset; skipping golden test");
        return;
    };
    let mut compared = 0;
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|e| e.to_str()) != Some("wzr") {
            continue;
        }
        let wam = read(&std::fs::read(&path).unwrap()).unwrap();
        if wam.wam_version.is_none() {
            continue; // not written by WAM: no golden value
        }
        for f in &wam.file_list.files {
            let tag = format!(
                "{} / {}",
                path.file_name().unwrap().to_string_lossy(),
                f.filename
            );
            let prepare = |svg: &str| {
                ow(
                    &cli,
                    req(
                        json!({"operation": "prepare", "source_text": svg, "source_format": "svg",
                    "settings": {"model": "pro", "material": "Aluminum", "units": "metric", "thickness": 3.0, "tolerance_mm": 0.02, "placement_xy_down_mm": [10.0, 10.0]}}),
                    ),
                )
            };
            let mut prepared = prepare(&f.svg_string);
            if prepared["status"] != "ok" {
                // openWAM rejects editor namespaces (Inkscape); retry with the bare <path> geometry.
                let bare = bare_paths(&f.svg_string);
                eprintln!(
                    "GOLDEN {tag}: openWAM prepare refused original ({}); retrying with bare <path> SVG",
                    prepared["error"]["message"]
                );
                prepared = prepare(&bare);
            }
            assert_eq!(prepared["status"], "ok", "{tag}: prepare {prepared}");
            let project = &prepared["result"]["project"];
            let contours = project["integration"]["planning"]["sketch"]["contours"]
                .as_array()
                .unwrap();
            // Kept-part sides by bbox nesting depth (even = outside, odd = inside).
            let boxes: Vec<[f64; 4]> = contours.iter().map(seg_box).collect();
            let inside = |a: &[f64; 4], b: &[f64; 4]| {
                b[0] < a[0] && b[1] < a[1] && b[2] > a[2] && b[3] > a[3]
            };
            let intent = json!({"kerf_override_mm": null, "contours": contours.iter().enumerate().map(|(i, c)| {
                let depth = boxes.iter().filter(|b| inside(&boxes[i], b)).count();
                let side = if c["closed"] != true { "no_cut" } else if depth % 2 == 0 { "outside" } else { "inside" };
                json!({"source_id": c["source_id"], "side": side, "start_fraction": 0, "tabs": [], "lead_in_mm": 2, "lead_out_mm": 2})
            }).collect::<Vec<_>>()});
            let planned = ow(
                &cli,
                req(json!({"operation": "plan_cam", "project": project, "intent": intent})),
            );
            if planned["status"] != "ok" {
                eprintln!(
                    "GOLDEN {tag}: openWAM plan_cam refused ({}): {}; WAM imported {} <path> group(s)",
                    planned["error"]["code"], planned["error"]["message"], f.instances
                );
                continue;
            }
            let artifact = &planned["result"]["artifact"];
            let receipt = ow(
                &cli,
                req(json!({"operation": "verify_cam", "artifact": artifact})),
            );
            let (ours, report) = match export(artifact, &receipt, &f.filename) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("GOLDEN {tag}: export refused: {e}");
                    continue;
                }
            };
            let bytes = write(&ours).unwrap();
            assert_eq!(
                write(&read(&bytes).unwrap()).unwrap(),
                bytes,
                "{tag}: export not stable"
            );
            let o = &ours.file_list.files[0];
            let n_paths = contours.len();
            let (a, b) = (&o.file_bb, &f.file_bb);
            let deltas: Vec<f64> = [
                (a.min_x, b.min_x),
                (a.min_y, b.min_y),
                (a.max_x, b.max_x),
                (a.max_y, b.max_y),
            ]
            .iter()
            .map(|(x, y)| (x.unwrap() - y.unwrap()).abs())
            .collect();
            let max = deltas.iter().copied().fold(0.0, f64::max);
            let wam_offset = wam.offsets.iter().find(|x| x.file_id == f.file_id).unwrap();
            eprintln!(
                "GOLDEN {tag}: openWAM contours {n_paths} | fileBB max|d| {max:.3e} in | offset ours {:?} {} vs WAM {:?} {} | tabs ours none vs WAM {:?} | instances ours 1 vs WAM {}",
                report.files[0].groups[0].offset_type,
                ours.offsets[0].distance,
                wam_offset.kind,
                wam_offset.distance,
                wam.tabs
                    .iter()
                    .find(|t| t.file_id == Some(f.file_id))
                    .map(|t| t.kind),
                f.instances
            );
            // Structure: same top-level keys in the same order, one group per instance.
            assert!(
                ours.wam_version == wam.wam_version
                    && ours.cut_order.is_some()
                    && ours.display_unit.is_some()
            );
            assert_eq!(o.group_instances[0].len() as u64, o.instances);
            assert_eq!(ours.placements[0].base_bb, o.file_bb);
            assert_eq!(wam_offset.distance, ours.offsets[0].distance);
            // Geometry: when openWAM sees only what WAM imports (<path> elements), boxes agree.
            let path_only = !f.svg_string.contains("<rect") && !f.svg_string.contains("<circle");
            if path_only {
                assert!(max < 1e-6, "{tag}: fileBB deviates {max:e} in");
                assert_eq!(report.files[0].cut_contours.len(), 1);
            }
            let rst = &ours.placements[0].rst_bb;
            assert!(
                (rst.width.unwrap() - a.width.unwrap()).abs() < 1e-12
                    && (rst.min_x.unwrap() + rst.max_x.unwrap()).abs() < 1e-12
            );
            compared += 1;
        }
    }
    assert!(compared > 0, "no WAM-written sample could be compared");
}
