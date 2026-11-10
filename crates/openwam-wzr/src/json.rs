//! Byte-exact `.wzr` (de)serialisation.
//!
//! WAM writes `JSON.stringify(project)`: compact separators, UTF-8, no BOM, no
//! trailing newline, and ECMAScript `Number::toString` for every number. The
//! only difference from `serde_json`'s compact output is number spelling
//! (`150` vs `150.0`, `1e+21` vs `1e21`), so a `Formatter` that spells f64 the ECMAScript way gives identical bytes.

use crate::model::Project;
use serde::Serialize;
use std::io;

#[derive(Debug, thiserror::Error)]
pub enum WzrError {
    #[error("file starts with a UTF-8 BOM; WAM never writes one")]
    Bom,
    #[error("not valid UTF-8")]
    Utf8,
    #[error("JSON does not match the WAM 2.4 .wzr schema: {0}")]
    Schema(#[source] serde_json::Error),
    #[error("non-finite number cannot be written to a .wzr")]
    NonFinite,
    #[error("inconsistent project: {0}")]
    Inconsistent(String),
    #[error("i/o: {0}")]
    Io(#[from] io::Error),
}

/// ECMAScript `Number.prototype.toString()` for finite `v` (ECMA-262 §6.1.6.1.20).
pub fn js_number(v: f64) -> Option<String> {
    if !v.is_finite() {
        return None;
    }
    if v == 0.0 {
        return Some("0".into());
    }
    let sign = if v < 0.0 { "-" } else { "" };
    // Rust's `{:e}` is the shortest round-tripping digit string, like ECMAScript.
    let sci = format!("{:e}", v.abs());
    let (mant, exp) = sci.split_once('e')?;
    let exp: i32 = exp.parse().ok()?;
    let digits: String = mant.chars().filter(|c| *c != '.').collect();
    let k = digits.len() as i32;
    let n = exp + 1;
    let body = if k <= n && n <= 21 {
        format!("{digits}{}", "0".repeat((n - k) as usize))
    } else if 0 < n && n <= 21 {
        format!("{}.{}", &digits[..n as usize], &digits[n as usize..])
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", "0".repeat((-n) as usize))
    } else {
        let e = n - 1;
        let es = if e >= 0 {
            format!("+{e}")
        } else {
            e.to_string()
        };
        if k == 1 {
            format!("{digits}e{es}")
        } else {
            format!("{}.{}e{es}", &digits[..1], &digits[1..])
        }
    };
    Some(format!("{sign}{body}"))
}

#[derive(Debug, Default)]
struct JsFormatter;

impl serde_json::ser::Formatter for JsFormatter {
    fn write_f64<W: ?Sized + io::Write>(&mut self, w: &mut W, v: f64) -> io::Result<()> {
        let s =
            js_number(v).ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "non-finite"))?;
        w.write_all(s.as_bytes())
    }
    fn write_f32<W: ?Sized + io::Write>(&mut self, w: &mut W, v: f32) -> io::Result<()> {
        self.write_f64(w, f64::from(v))
    }
}

/// Serialise any value exactly as `JSON.stringify` would.
pub fn to_js_json<T: Serialize>(value: &T) -> Result<Vec<u8>, WzrError> {
    let mut out = Vec::new();
    let mut ser = serde_json::Serializer::with_formatter(&mut out, JsFormatter);
    value.serialize(&mut ser).map_err(|e| {
        if e.is_io() {
            WzrError::NonFinite
        } else {
            WzrError::Schema(e)
        }
    })?;
    Ok(out)
}

/// Parse a `.wzr` file strictly (schema + cross-reference checks).
pub fn read(bytes: &[u8]) -> Result<Project, WzrError> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return Err(WzrError::Bom);
    }
    let text = std::str::from_utf8(bytes).map_err(|_| WzrError::Utf8)?;
    let project: Project = serde_json::from_str(text).map_err(WzrError::Schema)?;
    crate::check::consistency(&project)?;
    Ok(project)
}

/// Write a project exactly as WAM 2.4 would.
pub fn write(project: &Project) -> Result<Vec<u8>, WzrError> {
    crate::check::consistency(project)?;
    if !crate::check::all_finite(project) {
        return Err(WzrError::NonFinite);
    }
    to_js_json(project)
}

#[cfg(test)]
mod tests {
    use super::js_number;

    #[test]
    fn ecmascript_number_spelling() {
        let cases: &[(f64, &str)] = &[
            (150.0, "150"),
            (0.5, "0.5"),
            (-0.0, "0"),
            (26.20000000000001, "26.20000000000001"),
            (3.158660000224245e-8, "3.158660000224245e-8"),
            (0.000001, "0.000001"),
            (0.0000001, "1e-7"),
            (1e21, "1e+21"),
            (123456789012345680000.0, "123456789012345680000"),
            (-1.3500338699320231, "-1.3500338699320231"),
            (0.005208333333333333, "0.005208333333333333"),
            (1.5e300, "1.5e+300"),
        ];
        for (v, want) in cases {
            assert_eq!(js_number(*v).as_deref(), Some(*want), "{v:e}");
        }
        assert_eq!(js_number(f64::NAN), None);
    }
}
