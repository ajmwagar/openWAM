use reversal_waterjet_control::simulation::{SCENARIOS, run_scenario};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let scenarios: Vec<&str> = match args.as_slice() {
        [] => SCENARIOS.to_vec(),
        [arg] if arg == "--list" => {
            for name in SCENARIOS {
                println!("{name}");
            }
            return Ok(());
        }
        [flag, name] if flag == "--scenario" => vec![name.as_str()],
        _ => return Err("usage: plant [--list | --scenario NAME]".into()),
    };
    // Compute and validate the entire requested suite before emitting a report.
    let reports: Vec<_> = scenarios
        .into_iter()
        .map(run_scenario)
        .collect::<Result<_, _>>()
        .map_err(|e| format!("simulation failed: {e:?}"))?;
    println!(
        "scenario\tevent\tt_ms\tstate\trequested_jet\teffective_jet\tpressure_units\tnozzle_flow\twatchdog_tripped\tcutoff_failure\tunsafe_recovery\toutcome"
    );
    for report in reports {
        for (event, s) in report.trace {
            println!(
                "{}\t{}\t{}\t{:?}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{:?}",
                report.scenario,
                event,
                s.at_ms,
                s.state,
                s.requested.jet,
                s.effective.jet,
                s.pressure_units,
                s.nozzle_flow,
                s.watchdog_tripped,
                s.cutoff_failure,
                s.unsafe_recovery,
                report.outcome
            );
        }
    }
    eprintln!(
        "Synthetic model only; expected hazard demonstrations are not hardware qualification."
    );
    Ok(())
}
