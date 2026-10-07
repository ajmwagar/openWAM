//! Stream comparisons and project bindings; source/settings pairing is not inferred.
use crate::openwam_api::{ApiError, ErrorCode, ResearchCandidate};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::{wazer, wazer_compare};

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct StreamComparison {
    pub schema: String,
    pub reference_sha256: String,
    pub candidate_sha256: String,
    pub comparison: wazer_compare::Comparison,
}
#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResearchComparison {
    pub schema: String,
    pub profile: wazer::Profile,
    pub project_sha256: String,
    pub source_sha256: String,
    pub tables_canonical_sha256: String,
    pub streams: StreamComparison,
    pub source_settings_equivalence_established: bool,
    pub machine_output_enabled: bool,
    pub blockers: Vec<String>,
}

pub fn streams(
    reference: &str,
    candidate: &str,
    tolerances: wazer_compare::Tolerances,
) -> Result<StreamComparison, ApiError> {
    let comparison = wazer_compare::compare(reference, candidate, tolerances)
        .map_err(|e| ApiError::new(ErrorCode::ComparisonFailed, e.to_string()))?;
    Ok(StreamComparison {
        schema: "reversal.wazer-stream-comparison.v1".into(),
        reference_sha256: format!("{:x}", Sha256::digest(reference.as_bytes())),
        candidate_sha256: format!("{:x}", Sha256::digest(candidate.as_bytes())),
        comparison,
    })
}

pub fn research(
    candidate: ResearchCandidate,
    reference: &str,
    tolerances: wazer_compare::Tolerances,
) -> Result<ResearchComparison, ApiError> {
    let streams = streams(reference, &candidate.code.gcode, tolerances)?;
    let mut blockers = candidate.code.blockers;
    blockers.push("Supplied reference bytes do not establish WAM provenance or matching source and cut settings; comparison is limited to ordered observed command streams.".into());
    Ok(ResearchComparison {
        schema: "reversal.wazer-research-comparison.v1".into(),
        profile: candidate.code.profile,
        project_sha256: candidate.project_sha256,
        source_sha256: candidate.source_sha256,
        tables_canonical_sha256: candidate.tables_canonical_sha256,
        streams,
        source_settings_equivalence_established: false,
        machine_output_enabled: false,
        blockers,
    })
}
