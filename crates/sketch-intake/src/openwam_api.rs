//! Stateless agent boundary, shared by CLI and future frontend transports.
use crate::{MAX_INPUT_BYTES, bridge, cutting, openwam, parity, simulation};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use swarf_waterjet::{wazer, wazer_compare};

#[derive(Debug, Serialize, Deserialize, JsonSchema, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct ResearchCandidate {
    pub schema: String,
    pub project_sha256: String,
    pub source_sha256: String,
    pub tables_canonical_sha256: String,
    pub code: wazer::ResearchCode,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct GcodeInspection {
    pub source_sha256: String,
    pub canonical_gcode: String,
    pub program: wazer::Program,
    pub summary: wazer::Summary,
}

pub const REQUEST_SCHEMA: &str = "reversal.openwam-request.v1";
pub const RESPONSE_SCHEMA: &str = "reversal.openwam-response.v1";
pub const MAX_REQUEST_BYTES: usize = openwam::MAX_PROJECT_BYTES + 2 * 1024 * 1024;
pub const MAX_RESPONSE_BYTES: usize = 2 * MAX_REQUEST_BYTES;

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub model: cutting::Model,
    pub material: String,
    pub units: cutting::Units,
    pub thickness: f64,
    pub tolerance_mm: f64,
    pub placement_xy_down_mm: [f64; 2],
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub request: Operation,
}

#[derive(Debug, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Operation {
    ExportCamLinuxcnc {
        artifact: crate::cam::Artifact,
        profile: swarf_waterjet::linuxcnc::Profile,
    },
    VerifyCamLinuxcnc {
        artifact: crate::cam::Artifact,
        candidate: Box<crate::cam_loop::LinuxCncCandidate>,
    },
    SimulateCamGcodeShutdown {
        artifact: crate::cam::Artifact,
        candidate: Box<crate::cam_loop::Candidate>,
        fault: simulation::FaultSpec,
        completion: crate::gcode_simulation::CompletionSettings,
    },
    VerifyCamGcodeShutdown {
        artifact: crate::cam::Artifact,
        candidate: Box<crate::cam_loop::Candidate>,
        fault: simulation::FaultSpec,
        completion: crate::gcode_simulation::CompletionSettings,
        replay: serde_json::Value,
    },
    ReplayShutdown {
        input: crate::shutdown::Input,
    },
    VerifyShutdownReplay {
        input: crate::shutdown::Input,
        replay: serde_json::Value,
    },
    CompareShutdownTrace {
        input: crate::shutdown::Input,
        reference: Vec<crate::shutdown::TraceEntry>,
    },
    SimulateCamGcode {
        artifact: crate::cam::Artifact,
        candidate: Box<crate::cam_loop::Candidate>,
        fault: simulation::FaultSpec,
    },
    VerifyCamGcodeSimulation {
        artifact: crate::cam::Artifact,
        candidate: Box<crate::cam_loop::Candidate>,
        replay: serde_json::Value,
    },
    SimulateCam {
        artifact: crate::cam::Artifact,
        fault: simulation::FaultSpec,
    },
    VerifyCamSimulation {
        artifact: crate::cam::Artifact,
        replay: serde_json::Value,
    },
    ResearchCamGcode {
        artifact: crate::cam::Artifact,
        profile: wazer::Profile,
    },
    VerifyCamGcode {
        artifact: crate::cam::Artifact,
        candidate: Box<crate::cam_loop::Candidate>,
    },
    PlanCam {
        project: openwam::Project,
        intent: swarf_waterjet::cam::Intent,
    },
    VerifyCam {
        artifact: crate::cam::Artifact,
    },
    PreviewCam {
        artifact: crate::cam::Artifact,
    },
    CompareGcode {
        reference_gcode: String,
        candidate_gcode: String,
        tolerances: wazer_compare::Tolerances,
    },
    CompareResearchGcode {
        project: openwam::Project,
        profile: wazer::Profile,
        reference_gcode: String,
        tolerances: wazer_compare::Tolerances,
    },
    VerifyResearchComparison {
        project: openwam::Project,
        reference_gcode: String,
        comparison: Box<parity::ResearchComparison>,
    },
    InspectGcode {
        gcode_text: String,
    },
    ResearchGcode {
        project: openwam::Project,
        profile: wazer::Profile,
    },
    VerifyResearchGcode {
        project: openwam::Project,
        candidate: Box<ResearchCandidate>,
    },
    Catalog {
        model: Option<cutting::Model>,
        material: Option<String>,
        units: Option<cutting::Units>,
    },
    Prepare {
        source_text: String,
        source_format: bridge::SourceFormat,
        settings: Settings,
    },
    Revise {
        project: openwam::Project,
        settings: Settings,
    },
    Verify {
        project: openwam::Project,
    },
    Preview {
        project: openwam::Project,
    },
    Simulate {
        project: openwam::Project,
        fault: simulation::FaultSpec,
    },
    VerifySimulation {
        project: openwam::Project,
        replay: serde_json::Value,
    },
}

#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Output {
    CamLinuxcncGcode {
        candidate: Box<crate::cam_loop::LinuxCncCandidate>,
    },
    CamGcodeShutdownSimulation {
        replay: Box<crate::gcode_simulation::ShutdownReplay>,
    },
    ShutdownReplay {
        replay: Box<crate::shutdown::Replay>,
    },
    ShutdownComparison {
        comparison: serde_json::Value,
    },
    CamGcodeSimulation {
        replay: Box<crate::gcode_simulation::Replay>,
    },
    CamSimulation {
        replay: Box<crate::cam_loop::Replay>,
    },
    CamResearchGcode {
        candidate: Box<crate::cam_loop::Candidate>,
    },
    Cam {
        artifact: Box<crate::cam::Artifact>,
    },
    GcodeComparison {
        comparison: Box<parity::StreamComparison>,
    },
    ResearchComparison {
        comparison: Box<parity::ResearchComparison>,
    },
    GcodeInspection {
        inspection: GcodeInspection,
    },
    ResearchGcode {
        candidate: Box<ResearchCandidate>,
    },
    Catalog {
        rows: Vec<cutting::Row>,
        tables_canonical_sha256: String,
        materials_source: cutting::Source,
        specs_source: cutting::Source,
    },
    Project {
        project: openwam::Project,
    },
    Verification {
        verification: serde_json::Value,
    },
    Preview {
        media_type: String,
        content: String,
    },
    Simulation {
        replay: simulation::Replay,
    },
}

#[derive(Clone, Copy, Debug, Serialize, JsonSchema, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    CamInvalid,
    Usage,
    IoFailed,
    InputTooLarge,
    InvalidRequest,
    UnsupportedSchema,
    CatalogInvalid,
    InvalidProject,
    PlanningFailed,
    SimulationFailed,
    InvalidReplay,
    OutputTooLarge,
    GcodeInvalid,
    PostprocessingFailed,
    InvalidCandidate,
    ComparisonFailed,
    InvalidComparison,
}
#[derive(Debug, Serialize, JsonSchema)]
pub struct ApiError {
    pub code: ErrorCode,
    pub message: String,
}
impl ApiError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
    fn from_error(code: ErrorCode, error: anyhow::Error) -> Self {
        Self::new(code, format!("{error:#}"))
    }
}
#[derive(Debug, Serialize, JsonSchema)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Response {
    Ok {
        schema: String,
        result: Output,
        machine_output_enabled: bool,
    },
    Error {
        schema: String,
        error: ApiError,
        machine_output_enabled: bool,
    },
}
impl Response {
    pub fn error(error: ApiError) -> Self {
        Self::Error {
            schema: RESPONSE_SCHEMA.into(),
            error,
            machine_output_enabled: false,
        }
    }
    pub fn is_ok(&self) -> bool {
        matches!(self, Self::Ok { .. })
    }
}

/// Shared preparation API: parameters are explicit; no UI table indices or defaults.
pub fn prepare(
    source: &[u8],
    format: bridge::SourceFormat,
    settings: &Settings,
    catalog: &cutting::Catalog,
) -> anyhow::Result<openwam::Project> {
    let report = bridge::integrate_source(
        source,
        format,
        catalog,
        bridge::Selection {
            model: settings.model,
            material: &settings.material,
            units: settings.units,
            thickness: settings.thickness,
        },
        settings.tolerance_mm,
        settings.placement_xy_down_mm,
    )?;
    openwam::create(source, &report)
}

fn project_bytes(project: &openwam::Project) -> Result<Vec<u8>, ApiError> {
    let bytes = serde_json::to_vec(project)
        .map_err(|e| ApiError::new(ErrorCode::InvalidProject, e.to_string()))?;
    if bytes.len() > openwam::MAX_PROJECT_BYTES {
        return Err(ApiError::new(
            ErrorCode::InputTooLarge,
            "project exceeds 64 MiB",
        ));
    }
    Ok(bytes)
}

fn research_candidate(
    project: &openwam::Project,
    profile: wazer::Profile,
    catalog: &cutting::Catalog,
) -> Result<ResearchCandidate, ApiError> {
    let bytes = project_bytes(project)?;
    let (canonical, report) = openwam::reopen(&bytes, catalog)
        .map_err(|e| ApiError::from_error(ErrorCode::InvalidProject, e))?;
    if profile == wazer::Profile::Wam24ProPresegmented
        && report.planning.selected_row.model != cutting::Model::Pro
    {
        return Err(ApiError::new(
            ErrorCode::PostprocessingFailed,
            "WAM2.4 Pro service profile requires a Pro project",
        ));
    }
    // Rebuild the canonical typed report before hashing, so JS numeric
    // spelling changes cannot change project identity after verification.
    let bytes = project_bytes(&canonical)?;
    Ok(ResearchCandidate {
        schema: "reversal.wazer-research-candidate.v1".into(),
        project_sha256: format!("{:x}", Sha256::digest(&bytes)),
        source_sha256: report.transmog.source_sha256,
        tables_canonical_sha256: report.planning.tables_canonical_sha256,
        code: wazer::research_post(&report.swarf, profile)
            .map_err(|e| ApiError::new(ErrorCode::PostprocessingFailed, e.to_string()))?,
    })
}

pub fn execute(request: Request, catalog: &cutting::Catalog) -> Result<Output, ApiError> {
    if request.schema != REQUEST_SCHEMA {
        return Err(ApiError::new(
            ErrorCode::UnsupportedSchema,
            "unsupported OpenWAM request schema",
        ));
    }
    catalog
        .validate()
        .map_err(|e| ApiError::from_error(ErrorCode::CatalogInvalid, e))?;
    match request.request {
        Operation::SimulateCamGcodeShutdown {
            artifact,
            candidate,
            fault,
            completion,
        } => crate::gcode_simulation::simulate_shutdown(
            &artifact,
            catalog,
            &candidate,
            fault,
            &completion,
        )
        .map(|replay| Output::CamGcodeShutdownSimulation {
            replay: Box::new(replay),
        })
        .map_err(|e| ApiError::from_error(ErrorCode::SimulationFailed, e)),
        Operation::VerifyCamGcodeShutdown {
            artifact,
            candidate,
            fault,
            completion,
            replay,
        } => crate::gcode_simulation::verify_shutdown(
            &artifact,
            catalog,
            &candidate,
            fault,
            &completion,
            &replay,
        )
        .map(|verification| Output::Verification { verification })
        .map_err(|e| ApiError::from_error(ErrorCode::InvalidReplay, e)),
        Operation::ReplayShutdown { input } => crate::shutdown::simulate(&input)
            .map(|replay| Output::ShutdownReplay {
                replay: Box::new(replay),
            })
            .map_err(|e| ApiError::from_error(ErrorCode::SimulationFailed, e)),
        Operation::VerifyShutdownReplay { input, replay } => {
            crate::shutdown::verify(&input, &replay)
                .map(|verification| Output::Verification { verification })
                .map_err(|e| ApiError::from_error(ErrorCode::InvalidReplay, e))
        }
        Operation::CompareShutdownTrace { input, reference } => {
            crate::shutdown::compare(&input, &reference)
                .map(|comparison| Output::ShutdownComparison { comparison })
                .map_err(|e| ApiError::from_error(ErrorCode::ComparisonFailed, e))
        }
        Operation::SimulateCamGcode {
            artifact,
            candidate,
            fault,
        } => crate::gcode_simulation::simulate(&artifact, catalog, &candidate, fault)
            .map(|replay| Output::CamGcodeSimulation {
                replay: Box::new(replay),
            })
            .map_err(|e| ApiError::from_error(ErrorCode::SimulationFailed, e)),
        Operation::VerifyCamGcodeSimulation {
            artifact,
            candidate,
            replay,
        } => crate::gcode_simulation::verify(&artifact, catalog, &candidate, &replay)
            .map(|verification| Output::Verification { verification })
            .map_err(|e| ApiError::from_error(ErrorCode::InvalidReplay, e)),
        Operation::SimulateCam { artifact, fault } => {
            crate::cam_loop::simulate(&artifact, catalog, fault)
                .map(|replay| Output::CamSimulation {
                    replay: Box::new(replay),
                })
                .map_err(|e| ApiError::from_error(ErrorCode::SimulationFailed, e))
        }
        Operation::VerifyCamSimulation { artifact, replay } => {
            crate::cam_loop::verify_simulation(&artifact, catalog, &replay)
                .map(|verification| Output::Verification { verification })
                .map_err(|e| ApiError::from_error(ErrorCode::InvalidReplay, e))
        }
        Operation::ResearchCamGcode { artifact, profile } => {
            crate::cam_loop::research_gcode(&artifact, catalog, profile)
                .map(|candidate| Output::CamResearchGcode {
                    candidate: Box::new(candidate),
                })
                .map_err(|e| ApiError::from_error(ErrorCode::PostprocessingFailed, e))
        }
        Operation::ExportCamLinuxcnc { artifact, profile } => {
            crate::cam_loop::linuxcnc_gcode(&artifact, catalog, profile)
                .map(|candidate| Output::CamLinuxcncGcode {
                    candidate: Box::new(candidate),
                })
                .map_err(|e| ApiError::from_error(ErrorCode::PostprocessingFailed, e))
        }
        Operation::VerifyCamLinuxcnc {
            artifact,
            candidate,
        } => crate::cam_loop::verify_linuxcnc_gcode(&artifact, catalog, &candidate)
            .map(|verification| Output::Verification { verification })
            .map_err(|e| ApiError::from_error(ErrorCode::InvalidCandidate, e)),
        Operation::VerifyCamGcode {
            artifact,
            candidate,
        } => crate::cam_loop::verify_gcode(&artifact, catalog, &candidate)
            .map(|verification| Output::Verification { verification })
            .map_err(|e| ApiError::from_error(ErrorCode::InvalidCandidate, e)),
        Operation::PlanCam { project, intent } => crate::cam::prepare(&project, intent, catalog)
            .map(|artifact| Output::Cam {
                artifact: Box::new(artifact),
            })
            .map_err(|e| ApiError::from_error(ErrorCode::CamInvalid, e)),
        Operation::VerifyCam { artifact } => crate::cam::verify(&artifact, catalog)
            .map(|verification| Output::Verification { verification })
            .map_err(|e| ApiError::from_error(ErrorCode::CamInvalid, e)),
        Operation::PreviewCam { artifact } => crate::cam::preview(&artifact, catalog)
            .map(|svg| Output::Preview {
                media_type: "image/svg+xml".into(),
                content: svg,
            })
            .map_err(|e| ApiError::from_error(ErrorCode::CamInvalid, e)),
        Operation::CompareGcode {
            reference_gcode,
            candidate_gcode,
            tolerances,
        } => parity::streams(&reference_gcode, &candidate_gcode, tolerances).map(|comparison| {
            Output::GcodeComparison {
                comparison: Box::new(comparison),
            }
        }),
        Operation::CompareResearchGcode {
            project,
            profile,
            reference_gcode,
            tolerances,
        } => {
            let candidate = research_candidate(&project, profile, catalog)?;
            parity::research(candidate, &reference_gcode, tolerances).map(|comparison| {
                Output::ResearchComparison {
                    comparison: Box::new(comparison),
                }
            })
        }
        Operation::VerifyResearchComparison {
            project,
            reference_gcode,
            comparison,
        } => {
            let candidate = research_candidate(&project, comparison.profile, catalog)?;
            let rebuilt = parity::research(
                candidate,
                &reference_gcode,
                comparison.streams.comparison.tolerances,
            )?;
            if *comparison != rebuilt {
                return Err(ApiError::new(
                    ErrorCode::InvalidComparison,
                    "comparison differs from independently rebuilt project, reference bytes, or stream analysis",
                ));
            }
            Ok(Output::Verification {
                verification: serde_json::json!({
                    "schema": "reversal.wazer-comparison-verification.v1", "verified": true,
                    "project_sha256": rebuilt.project_sha256,
                    "reference_sha256": rebuilt.streams.reference_sha256,
                    "candidate_sha256": rebuilt.streams.candidate_sha256,
                    "stream_verdict": rebuilt.streams.comparison.verdict,
                    "source_settings_equivalence_established": false,
                    "machine_output_enabled": false, "controller_qualified": false,
                }),
            })
        }
        Operation::InspectGcode { gcode_text } => {
            let program = wazer::parse(&gcode_text)
                .map_err(|e| ApiError::new(ErrorCode::GcodeInvalid, e.to_string()))?;
            let summary = wazer::validate(&program)
                .map_err(|e| ApiError::new(ErrorCode::GcodeInvalid, e.to_string()))?;
            let canonical_gcode = wazer::serialize(&program)
                .map_err(|e| ApiError::new(ErrorCode::GcodeInvalid, e.to_string()))?;
            Ok(Output::GcodeInspection {
                inspection: GcodeInspection {
                    source_sha256: format!("{:x}", Sha256::digest(gcode_text.as_bytes())),
                    canonical_gcode,
                    program,
                    summary,
                },
            })
        }
        Operation::ResearchGcode { project, profile } => {
            research_candidate(&project, profile, catalog).map(|candidate| Output::ResearchGcode {
                candidate: Box::new(candidate),
            })
        }
        Operation::VerifyResearchGcode { project, candidate } => {
            let rebuilt = research_candidate(&project, candidate.code.profile, catalog)?;
            if *candidate != rebuilt {
                return Err(ApiError::new(
                    ErrorCode::InvalidCandidate,
                    "candidate differs from independently rebuilt project, tables, or postprocessor",
                ));
            }
            Ok(Output::Verification {
                verification: serde_json::json!({
                    "schema": "reversal.wazer-research-verification.v1", "verified": true,
                    "project_sha256": rebuilt.project_sha256, "source_sha256": rebuilt.source_sha256,
                    "gcode_sha256": format!("{:x}", Sha256::digest(rebuilt.code.gcode.as_bytes())),
                    "machine_output_enabled": false, "controller_qualified": false,
                }),
            })
        }
        Operation::Catalog {
            model,
            material,
            units,
        } => Ok(Output::Catalog {
            tables_canonical_sha256: catalog
                .canonical_sha256()
                .map_err(|e| ApiError::from_error(ErrorCode::CatalogInvalid, e))?,
            materials_source: catalog.materials_source.clone(),
            specs_source: catalog.specs_source.clone(),
            rows: catalog
                .rows
                .iter()
                .filter(|r| {
                    model.is_none_or(|m| r.model == m)
                        && units.is_none_or(|u| r.units == u)
                        && material
                            .as_ref()
                            .is_none_or(|m| r.material.eq_ignore_ascii_case(m))
                })
                .cloned()
                .collect(),
        }),
        Operation::Prepare {
            source_text,
            source_format,
            settings,
        } => {
            if source_text.len() > MAX_INPUT_BYTES {
                return Err(ApiError::new(
                    ErrorCode::InputTooLarge,
                    "source exceeds 1 MiB",
                ));
            }
            prepare(source_text.as_bytes(), source_format, &settings, catalog)
                .map(|project| Output::Project { project })
                .map_err(|e| ApiError::from_error(ErrorCode::PlanningFailed, e))
        }
        Operation::Revise { project, settings } => {
            let (original, report) = openwam::reopen(&project_bytes(&project)?, catalog)
                .map_err(|e| ApiError::from_error(ErrorCode::InvalidProject, e))?;
            prepare(
                original.source_text.as_bytes(),
                report.source_format,
                &settings,
                catalog,
            )
            .map(|project| Output::Project { project })
            .map_err(|e| ApiError::from_error(ErrorCode::PlanningFailed, e))
        }
        Operation::Verify { project } => openwam::check(&project_bytes(&project)?, catalog)
            .map(|verification| Output::Verification { verification })
            .map_err(|e| ApiError::from_error(ErrorCode::InvalidProject, e)),
        Operation::Preview { project } => openwam::preview_svg(&project_bytes(&project)?, catalog)
            .map(|content| Output::Preview {
                media_type: "image/svg+xml".into(),
                content,
            })
            .map_err(|e| ApiError::from_error(ErrorCode::InvalidProject, e)),
        Operation::Simulate { project, fault } => {
            let (project, _) = openwam::reopen(&project_bytes(&project)?, catalog)
                .map_err(|e| ApiError::from_error(ErrorCode::InvalidProject, e))?;
            let integration = serde_json::to_vec(&project.integration)
                .map_err(|e| ApiError::new(ErrorCode::InvalidProject, e.to_string()))?;
            simulation::replay(project.source_text.as_bytes(), &integration, catalog, fault)
                .map(|replay| Output::Simulation { replay })
                .map_err(|e| ApiError::from_error(ErrorCode::SimulationFailed, e))
        }
        Operation::VerifySimulation { project, replay } => {
            let (project, _) = openwam::reopen(&project_bytes(&project)?, catalog)
                .map_err(|e| ApiError::from_error(ErrorCode::InvalidProject, e))?;
            simulation::verify(
                project.source_text.as_bytes(),
                &serde_json::to_vec(&project.integration)
                    .map_err(|e| ApiError::new(ErrorCode::InvalidProject, e.to_string()))?,
                &serde_json::to_vec(&replay)
                    .map_err(|e| ApiError::new(ErrorCode::InvalidReplay, e.to_string()))?,
                catalog,
            )
            .map(|verification| Output::Verification { verification })
            .map_err(|e| ApiError::from_error(ErrorCode::InvalidReplay, e))
        }
    }
}

pub fn dispatch(bytes: &[u8]) -> Response {
    if bytes.len() > MAX_REQUEST_BYTES {
        return Response::error(ApiError::new(
            ErrorCode::InputTooLarge,
            "request exceeds its byte budget",
        ));
    }
    let request = match serde_json::from_slice(bytes) {
        Ok(request) => request,
        Err(e) => return Response::error(ApiError::new(ErrorCode::InvalidRequest, e.to_string())),
    };
    let catalog = match cutting::bundled() {
        Ok(catalog) => catalog,
        Err(e) => return Response::error(ApiError::from_error(ErrorCode::CatalogInvalid, e)),
    };
    match execute(request, &catalog) {
        Ok(result) => Response::Ok {
            schema: RESPONSE_SCHEMA.into(),
            result,
            machine_output_enabled: false,
        },
        Err(error) => Response::error(error),
    }
}

pub fn capabilities() -> serde_json::Value {
    serde_json::json!({
        "schema": "reversal.openwam-capabilities.v1", "package_version": env!("CARGO_PKG_VERSION"),
        "request_schema": schemars::schema_for!(Request), "response_schema": schemars::schema_for!(Response),
        "request_version": REQUEST_SCHEMA, "response_version": RESPONSE_SCHEMA,
        "limits": {"request_bytes": MAX_REQUEST_BYTES, "response_bytes": MAX_RESPONSE_BYTES, "source_bytes": MAX_INPUT_BYTES, "project_bytes": openwam::MAX_PROJECT_BYTES,
            "shutdown_events":crate::shutdown::MAX_EVENTS, "shutdown_time_us":crate::shutdown::MAX_TIME_US, "shutdown_switch_bindings":crate::shutdown::MAX_SWITCHES},
        "machine_output_enabled": false, "hardware_qualified": false,
        "shutdown_example": serde_json::from_str::<serde_json::Value>(include_str!("../../../wazer/fixtures/shutdown-completion.request.json")).expect("valid retained shutdown example"),
        "prepare_example": Request { schema: REQUEST_SCHEMA.into(), request: Operation::Prepare {
            source_text: include_str!("../examples/plate.svg").into(), source_format: bridge::SourceFormat::Svg,
            settings: Settings { model: cutting::Model::Pro, material: "Aluminum".into(), units: cutting::Units::Metric, thickness: 3.0, tolerance_mm: 0.02, placement_xy_down_mm: [0.0, 0.0] },
        }},
    })
}
