//! `openwam-wzr`: read, round-trip and export WAM 2.4 `.wzr` project files.
//!
//! JSON on stdout, exit 0 on success, exit 2 with `{"status":"error",...}`.
//! `export_wzr` runs `openwam request -` (verify_cam) as a child process and
//! refuses unless that receipt verifies the same artifact. It writes a project
//! file only; it never drives, uploads to or talks to a machine.

use openwam_wzr::{capabilities, export, json, open, summary, svg};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode, Stdio};

const MAX_ARTIFACT_BYTES: u64 = 64 << 20;
const USAGE: &str = "usage: openwam-wzr capabilities | read FILE.wzr | roundtrip FILE.wzr | material FILE.wzr | open FILE.wzr | paths_only (FILE.svg|-) | export_wzr --artifact (FILE|-) --out FILE.wzr [--name NAME.svg] [--openwam PATH]";

struct Fail(&'static str, String);

fn fail(code: &'static str, e: impl std::fmt::Display) -> Fail {
    Fail(code, e.to_string())
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(v) => {
            println!("{v}");
            ExitCode::SUCCESS
        }
        Err(Fail(code, message)) => {
            println!(
                "{}",
                json!({"status": "error", "error": {"code": code, "message": message}})
            );
            ExitCode::from(2)
        }
    }
}

fn read_file(path: &str) -> Result<Vec<u8>, Fail> {
    let mut buf = Vec::new();
    if path == "-" {
        std::io::stdin()
            .take(MAX_ARTIFACT_BYTES + 1)
            .read_to_end(&mut buf)
            .map_err(|e| fail("io", e))?;
    } else {
        std::fs::File::open(path)
            .and_then(|f| f.take(MAX_ARTIFACT_BYTES + 1).read_to_end(&mut buf))
            .map_err(|e| fail("io", e))?;
    }
    if buf.len() as u64 > MAX_ARTIFACT_BYTES {
        return Err(fail("too_large", "input exceeds 64 MiB"));
    }
    Ok(buf)
}

fn run(args: &[String]) -> Result<Value, Fail> {
    match args.first().map(String::as_str) {
        Some("capabilities") => Ok(capabilities()),
        Some("read") => {
            let path = args.get(1).ok_or_else(|| fail("usage", USAGE))?;
            let project = json::read(&read_file(path)?).map_err(|e| fail("wzr_invalid", e))?;
            Ok(json!({"status": "ok", "summary": summary::summarize(&project)}))
        }
        Some("roundtrip") => {
            let path = args.get(1).ok_or_else(|| fail("usage", USAGE))?;
            let input = read_file(path)?;
            let project = json::read(&input).map_err(|e| fail("wzr_invalid", e))?;
            let output = json::write(&project).map_err(|e| fail("wzr_invalid", e))?;
            let first_diff = input
                .iter()
                .zip(&output)
                .position(|(a, b)| a != b)
                .or((input.len() != output.len()).then(|| input.len().min(output.len())));
            Ok(
                json!({"status": "ok", "identical": first_diff.is_none(), "bytes_in": input.len(),
                      "bytes_out": output.len(), "first_diff_offset": first_diff}),
            )
        }
        Some("material") => {
            let path = args.get(1).ok_or_else(|| fail("usage", USAGE))?;
            let project = json::read(&read_file(path)?).map_err(|e| fail("wzr_invalid", e))?;
            let origin = Path::new(path)
                .file_name()
                .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
            let material = open::material_recipe(&project, &origin)
                .map_err(|e| fail("material_invalid", e))?;
            Ok(json!({
                "status": "ok", "kind": "wzr_material", "material": material,
                "wzr_user_def": project.selected_material.material.user_def,
                "quality": project.cut_quality, "abrasive_mode": open::abrasive_mode(&project),
                "machine_output_enabled": false, "hardware_qualified": false,
            }))
        }
        Some("open") => {
            let path = args.get(1).ok_or_else(|| fail("usage", USAGE))?;
            let project = json::read(&read_file(path)?).map_err(|e| fail("wzr_invalid", e))?;
            let origin = Path::new(path)
                .file_name()
                .map_or_else(|| path.clone(), |n| n.to_string_lossy().into_owned());
            Ok(json!({
                "status": "ok", "kind": "wzr_open", "wam_version": project.wam_version,
                "files": open::designs(&project).map_err(|e| fail("wzr_invalid", e))?,
                "material": open::material_recipe(&project, &origin).map_err(|e| fail("material_invalid", e))?,
                "wzr_user_def": project.selected_material.material.user_def,
                "quality": project.cut_quality, "abrasive_mode": open::abrasive_mode(&project),
                "machine_output_enabled": false, "hardware_qualified": false,
            }))
        }
        Some("paths_only") => {
            let path = args.get(1).ok_or_else(|| fail("usage", USAGE))?;
            let text = String::from_utf8(read_file(path)?).map_err(|e| fail("svg_invalid", e))?;
            let r = svg::paths_only(&text).map_err(|e| fail("svg_invalid", e))?;
            Ok(
                json!({"status": "ok", "kind": "paths_only", "svg": r.svg, "paths": r.paths,
                "ignored_shapes": r.ignored_shapes, "machine_output_enabled": false, "hardware_qualified": false}),
            )
        }
        Some("export_wzr") => export_wzr(&args[1..]),
        _ => Err(fail("usage", USAGE)),
    }
}

fn flag<'a>(args: &'a [String], name: &str) -> Option<&'a str> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
}

fn openwam_cli(args: &[String]) -> Result<PathBuf, Fail> {
    if let Some(p) = flag(args, "--openwam")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("OPENWAM_CLI").map(PathBuf::from))
    {
        return Ok(p);
    }
    let sibling = std::env::current_exe()
        .map_err(|e| fail("io", e))?
        .with_file_name("openwam");
    if sibling.is_file() {
        Ok(sibling)
    } else {
        Err(fail(
            "usage",
            "openwam CLI not found: pass --openwam or set OPENWAM_CLI",
        ))
    }
}

/// Refuse removable media: export targets local disk only (never an SD card).
fn check_out_path(out: &Path) -> Result<(), Fail> {
    if out.extension().and_then(|e| e.to_str()) != Some("wzr") {
        return Err(fail("bad_output", "output must end in .wzr"));
    }
    let parent = out
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    let parent = parent
        .canonicalize()
        .map_err(|e| fail("bad_output", format!("{}: {e}", parent.display())))?;
    for removable in ["/run/media", "/media", "/mnt"] {
        if parent.starts_with(removable) {
            return Err(fail(
                "bad_output",
                format!("refusing to write to removable media under {removable}"),
            ));
        }
    }
    Ok(())
}

fn verify_cam(cli: &Path, artifact: &Value) -> Result<Value, Fail> {
    let request = json!({"schema": "reversal.openwam-request.v1", "request": {"operation": "verify_cam", "artifact": artifact}});
    let mut child = Command::new(cli)
        .args(["request", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| fail("verify_failed", format!("{}: {e}", cli.display())))?;
    let mut stdin = child
        .stdin
        .take()
        .ok_or_else(|| fail("verify_failed", "no stdin"))?;
    stdin
        .write_all(request.to_string().as_bytes())
        .map_err(|e| fail("verify_failed", e))?;
    drop(stdin);
    let out = child
        .wait_with_output()
        .map_err(|e| fail("verify_failed", e))?;
    serde_json::from_slice(&out.stdout)
        .map_err(|e| fail("verify_failed", format!("openwam emitted non-JSON: {e}")))
}

fn export_wzr(args: &[String]) -> Result<Value, Fail> {
    let artifact_path = flag(args, "--artifact").ok_or_else(|| fail("usage", USAGE))?;
    let out = PathBuf::from(flag(args, "--out").ok_or_else(|| fail("usage", USAGE))?);
    let name = flag(args, "--name").unwrap_or("openwam-part.svg");
    check_out_path(&out)?;
    let artifact: Value = serde_json::from_slice(&read_file(artifact_path)?)
        .map_err(|e| fail("artifact_invalid", e))?;
    // Accept either the bare artifact or the full plan_cam response.
    let artifact = if artifact["result"]["artifact"].is_object() {
        artifact["result"]["artifact"].clone()
    } else {
        artifact
    };
    let receipt = verify_cam(&openwam_cli(args)?, &artifact)?;
    let (project, report) = export::export(&artifact, &receipt, name).map_err(|e| match e {
        export::ExportError::NotVerified(_) => fail("not_verified", e),
        _ => fail("export_refused", e),
    })?;
    let bytes = json::write(&project).map_err(|e| fail("wzr_invalid", e))?;
    // Self-check: the written file must read back and re-write byte-identically.
    let again = json::read(&bytes)
        .and_then(|p| json::write(&p))
        .map_err(|e| fail("wzr_invalid", e))?;
    if again != bytes {
        return Err(fail(
            "wzr_invalid",
            "export failed its own round-trip check",
        ));
    }
    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&out)
        .map_err(|e| fail("io", format!("{}: {e}", out.display())))?;
    f.write_all(&bytes).map_err(|e| fail("io", e))?;
    let sha: String = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(json!({
        "status": "ok", "kind": "wzr_export", "path": out, "bytes": bytes.len(), "sha256": sha,
        "verify_cam": receipt["result"]["verification"], "report": report,
        "wzr_file_export": true, "machine_output_enabled": false, "hardware_qualified": false,
        "warning": "not hardware-qualified: test on scrap first",
    }))
}
