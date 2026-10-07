use anyhow::{Context, Result, ensure};
use reversal_sketch_intake::{
    MAX_INPUT_BYTES,
    cutting::{self, Model, Units},
    inspect_svg,
};
use std::{
    fs::File,
    io::{Read, Write},
};

fn main() -> Result<()> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.first().and_then(|v| v.to_str()) == Some("--openwam-preview") {
        ensure!(
            args.len() == 2,
            "usage: --openwam-preview project.openwam.json"
        );
        let html = reversal_sketch_intake::openwam::preview(
            &read(&args[1], reversal_sketch_intake::openwam::MAX_PROJECT_BYTES)?,
            &cutting::bundled()?,
        )?;
        std::io::stdout().lock().write_all(html.as_bytes())?;
        return Ok(());
    }
    let value = match args.first().and_then(|v| v.to_str()) {
        Some("--openwam-check" | "--openwam-simulate") => {
            ensure!(
                args.len() == 2,
                "usage: --openwam-check|--openwam-simulate project.openwam.json"
            );
            let bytes = read(&args[1], reversal_sketch_intake::openwam::MAX_PROJECT_BYTES)?;
            let catalog = cutting::bundled()?;
            if args[0] == "--openwam-check" {
                reversal_sketch_intake::openwam::check(&bytes, &catalog)?
            } else {
                serde_json::to_value(reversal_sketch_intake::openwam::simulate(&bytes, &catalog)?)?
            }
        }
        Some("--simulate-integration") => {
            ensure!(
                args.len() == 4 || args.len() == 5,
                "usage: --simulate-integration drawing.svg|dxf integration.json none|enclosure_open|watchdog_stall|stale_input|release_blocked|pressure_sensor_lies [fault_at_ms]"
            );
            use reversal_sketch_intake::simulation::{self, FaultKind, FaultSpec};
            let kind: FaultKind = serde_json::from_value(serde_json::Value::String(
                args[3].to_str().context("invalid fault kind")?.into(),
            ))?;
            ensure!(
                kind == FaultKind::None || args.len() == 5,
                "fault time is required"
            );
            let at_ms = if args.len() == 5 {
                args[4].to_str().context("invalid fault time")?.parse()?
            } else {
                0
            };
            serde_json::to_value(simulation::replay(
                &read(&args[1], MAX_INPUT_BYTES)?,
                &read(&args[2], 64 * 1024 * 1024)?,
                &cutting::bundled()?,
                FaultSpec { kind, at_ms },
            )?)?
        }
        Some("--verify-simulation") => {
            ensure!(
                args.len() == 4,
                "usage: --verify-simulation drawing.svg|dxf integration.json simulation.json"
            );
            reversal_sketch_intake::simulation::verify(
                &read(&args[1], MAX_INPUT_BYTES)?,
                &read(&args[2], 64 * 1024 * 1024)?,
                &read(&args[3], 16 * 1024 * 1024)?,
                &cutting::bundled()?,
            )?
        }
        Some("--verify-integration") => {
            ensure!(
                args.len() == 3,
                "usage: --verify-integration drawing.svg|dxf integration.json"
            );
            reversal_sketch_intake::bridge::verify_source(
                &read(&args[1], MAX_INPUT_BYTES)?,
                &read(&args[2], 64 * 1024 * 1024)?,
                &cutting::bundled()?,
            )?
        }
        Some("--integrate-wazer" | "--openwam-create") => {
            ensure!(
                args.len() == 9,
                "usage: --integrate-wazer|--openwam-create drawing.svg|dxf pro|desktop material thickness metric|imperial tolerance_mm placement_x_mm placement_y_down_mm"
            );
            use reversal_sketch_intake::bridge::{self, Selection, SourceFormat};
            let model = parse_model(&args[2])?;
            let units = parse_units(&args[5])?;
            let number = |i: usize| -> Result<f64> {
                Ok(args[i]
                    .to_str()
                    .context("invalid numeric argument")?
                    .parse()?)
            };
            let format = match std::path::Path::new(&args[1])
                .extension()
                .and_then(|v| v.to_str())
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("svg") => SourceFormat::Svg,
                Some("dxf") => SourceFormat::Dxf,
                _ => anyhow::bail!("explicit .svg or .dxf extension required"),
            };
            let original = read(&args[1], MAX_INPUT_BYTES)?;
            let integration = bridge::integrate_source(
                &original,
                format,
                &cutting::bundled()?,
                Selection {
                    model,
                    units,
                    material: args[3].to_str().context("invalid material")?,
                    thickness: number(4)?,
                },
                number(6)?,
                [number(7)?, number(8)?],
            )?;
            if args[0] == "--openwam-create" {
                serde_json::to_value(reversal_sketch_intake::openwam::create(
                    &original,
                    &integration,
                )?)?
            } else {
                serde_json::to_value(integration)?
            }
        }
        Some("--verify-wazer") => {
            ensure!(
                args.len() == 3,
                "usage: --verify-wazer drawing.svg job.json"
            );
            cutting::verify_job(
                &read(&args[1], MAX_INPUT_BYTES)?,
                &read(&args[2], 32 * 1024 * 1024)?,
                &cutting::bundled()?,
            )?
        }
        Some("--import-wazer") => {
            ensure!(
                args.len() == 4,
                "usage: --import-wazer YYYY-MM-DD materials.html specs.html"
            );
            serde_json::to_value(cutting::import_published(
                &read(&args[2], 2 * 1024 * 1024)?,
                &read(&args[3], 2 * 1024 * 1024)?,
                args[1].to_str().context("invalid date")?,
            )?)?
        }
        Some("--cutting-list") => {
            ensure!(args.len() <= 2, "usage: --cutting-list [material]");
            let catalog = cutting::bundled()?;
            let query = args
                .get(1)
                .and_then(|v| v.to_str())
                .unwrap_or("")
                .to_lowercase();
            serde_json::to_value(
                catalog
                    .rows
                    .iter()
                    .filter(|r| r.material.to_lowercase().contains(&query))
                    .collect::<Vec<_>>(),
            )?
        }
        Some("--plan-wazer") => {
            ensure!(
                args.len() == 6,
                "usage: --plan-wazer drawing.svg pro|desktop material thickness metric|imperial"
            );
            let model = parse_model(&args[2])?;
            let units = parse_units(&args[5])?;
            let thickness = args[4].to_str().context("invalid thickness")?.parse()?;
            let material = args[3].to_str().context("invalid material")?;
            let sketch = inspect_svg(&read(&args[1], MAX_INPUT_BYTES)?)?;
            serde_json::to_value(cutting::plan(
                sketch,
                &cutting::bundled()?,
                model,
                material,
                units,
                thickness,
            )?)?
        }
        _ => {
            ensure!(
                args.len() == 1,
                "usage: reversal-sketch-intake drawing.svg | --cutting-list [material] | --plan-wazer drawing.svg pro|desktop material thickness metric|imperial | --verify-wazer drawing.svg job.json | --import-wazer YYYY-MM-DD materials.html specs.html"
            );
            serde_json::to_value(inspect_svg(&read(&args[0], MAX_INPUT_BYTES)?)?)?
        }
    };
    let mut output = std::io::stdout().lock();
    if args.first().and_then(|v| v.to_str()) == Some("--openwam-create") {
        let bytes = serde_json::to_vec_pretty(&value)?;
        ensure!(
            bytes.len() <= reversal_sketch_intake::openwam::MAX_PROJECT_BYTES,
            "project exceeds 64 MiB"
        );
        output.write_all(&bytes)?;
    } else {
        serde_json::to_writer_pretty(&mut output, &value)?;
    }
    writeln!(output)?;
    Ok(())
}

fn read(path: &std::ffi::OsStr, limit: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    File::open(path)
        .context("opening input")?
        .take((limit + 1) as u64)
        .read_to_end(&mut bytes)?;
    ensure!(bytes.len() <= limit, "input exceeds size bound");
    Ok(bytes)
}

fn parse_model(value: &std::ffi::OsStr) -> Result<Model> {
    match value.to_str() {
        Some("pro") => Ok(Model::Pro),
        Some("desktop") => Ok(Model::Desktop),
        _ => anyhow::bail!("model must be pro or desktop"),
    }
}
fn parse_units(value: &std::ffi::OsStr) -> Result<Units> {
    match value.to_str() {
        Some("metric") => Ok(Units::Metric),
        Some("imperial") => Ok(Units::Imperial),
        _ => anyhow::bail!("units must be metric or imperial"),
    }
}
