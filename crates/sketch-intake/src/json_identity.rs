//! JSON numeric spelling is not geometry tolerance. Keep all other identity exact.
use serde_json::Value;
pub(crate) fn equivalent(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(a), Value::Number(b)) => {
            if a == b {
                return true;
            }
            // Only compare an integer against a float within the exactly
            // representable integer range. Distinct large integers stay distinct.
            let safe = |n: &serde_json::Number| {
                n.as_i64().is_some_and(|v| v.unsigned_abs() <= (1 << 53))
                    || n.as_u64().is_some_and(|v| v <= (1 << 53))
            };
            ((a.is_f64() && safe(b)) || (b.is_f64() && safe(a))) && a.as_f64() == b.as_f64()
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(a, b)| equivalent(a, b))
        }
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, a)| b.get(k).is_some_and(|b| equivalent(a, b)))
        }
        _ => a == b,
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn numeric_spelling_is_equivalent_without_tolerance_or_precision_loss() {
        let parse = |s| serde_json::from_str(s).unwrap();
        assert!(equivalent(&parse("[108,0,1]"), &parse("[108.0,-0.0,1e0]")));
        for (a, b) in [
            ("108", "108.00000000000001"),
            ("9007199254740993", "9007199254740992.0"),
            ("[1,2]", "[2,1]"),
            ("{\"x\":1}", "{\"x\":1,\"y\":0}"),
        ] {
            assert!(!equivalent(&parse(a), &parse(b)));
        }
    }
}
