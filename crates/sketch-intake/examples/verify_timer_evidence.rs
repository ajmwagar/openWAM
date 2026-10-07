//! Verify retained static RE captures against the original image, without execution.
use anyhow::{Context, Result, ensure};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

#[derive(Deserialize)]
struct Span {
    address: String,
    length: usize,
    sha256: String,
    ghidra_memory_capture: String,
}
#[derive(Deserialize)]
struct Capture {
    path: String,
    sha256: String,
}
#[derive(Deserialize)]
struct Receipt {
    schema: String,
    firmware_receipt: String,
    memory_spans: Vec<Span>,
    captures: Vec<Capture>,
}
#[derive(Deserialize)]
struct Firmware {
    bytes: usize,
    sha256: String,
    image_base: String,
}
#[derive(Deserialize)]
struct FirmwareReceipt {
    firmware: Firmware,
}
#[derive(Deserialize)]
struct Memory {
    address: String,
    length: usize,
    data: Vec<u8>,
}
fn hex(value: &str) -> Result<usize> {
    Ok(usize::from_str_radix(value.trim_start_matches("0x"), 16)?)
}
fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn source_span<'a>(image: &'a [u8], base: usize, span: &Span) -> Result<&'a [u8]> {
    let offset = hex(&span.address)?
        .checked_sub(base)
        .context("span precedes image base")?;
    let end = offset
        .checked_add(span.length)
        .context("span address overflow")?;
    let bytes = image.get(offset..end).context("span exceeds image")?;
    ensure!(digest(bytes) == span.sha256, "source span digest differs");
    Ok(bytes)
}
fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let path = args
        .next()
        .context("usage: verify_timer_evidence FIRMWARE")?;
    ensure!(args.next().is_none(), "expected exactly one firmware path");
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let receipt: Receipt =
        serde_json::from_slice(&fs::read(root.join("wazer/shared-shutdown-timer.json"))?)?;
    ensure!(
        receipt.schema == "openwam.shared-shutdown-timer-research.v1",
        "unsupported receipt schema"
    );
    let source: FirmwareReceipt =
        serde_json::from_slice(&fs::read(root.join(&receipt.firmware_receipt))?)?;
    let image = fs::read(path)?;
    ensure!(
        image.len() == source.firmware.bytes,
        "firmware size differs"
    );
    ensure!(
        digest(&image) == source.firmware.sha256,
        "firmware digest differs"
    );
    for capture in &receipt.captures {
        let bytes = fs::read(root.join(&capture.path))
            .with_context(|| format!("missing capture {}", capture.path))?;
        ensure!(
            digest(&bytes) == capture.sha256,
            "capture digest differs: {}",
            capture.path
        );
    }
    let base = hex(&source.firmware.image_base)?;
    for span in &receipt.memory_spans {
        let original = source_span(&image, base, span)?;
        let memory: Memory =
            serde_json::from_slice(&fs::read(root.join(&span.ghidra_memory_capture))?)?;
        ensure!(
            hex(&memory.address)? == hex(&span.address)?
                && memory.length == span.length
                && memory.data.as_slice() == original,
            "Ghidra memory differs at {}",
            span.address
        );
    }
    println!(
        "{}",
        serde_json::json!({"verified":true,"memory_spans":receipt.memory_spans.len(),
            "captures":receipt.captures.len(),"firmware_sha256":source.firmware.sha256,
            "firmware_executed":false})
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn source_span_rejects_tampering_and_bad_ranges() {
        let image = [10, 20, 30, 40];
        let mut span = Span {
            address: "0x4001".into(),
            length: 2,
            sha256: digest(&image[1..3]),
            ghidra_memory_capture: String::new(),
        };
        assert_eq!(source_span(&image, 0x4000, &span).unwrap(), &[20, 30]);
        assert!(source_span(&[10, 21, 30, 40], 0x4000, &span).is_err());
        span.address = "0x3fff".into();
        assert!(source_span(&image, 0x4000, &span).is_err());
        span.address = "0x4003".into();
        assert!(source_span(&image, 0x4000, &span).is_err());
        span.address = "0x4001".into();
        span.length = usize::MAX;
        assert!(source_span(&image, 0x4000, &span).is_err());
    }
}
