use reversal_waterjet_control::{Command, Controller, Inputs};

fn main() {
    let mut controller = Controller::new(50).expect("nonzero simulation budget");
    // A synthetic scenario, not a WAZER control sequence or calibrated timing.
    for (tick, command, enclosure, released) in [
        (0, Command::Arm, true, true),
        (1, Command::Pierce, true, false),
        (2, Command::Cut, true, false),
        (3, Command::Poll, false, false),
        (4, Command::Reset, true, false),
        (5, Command::Reset, true, true),
    ] {
        let inputs = Inputs {
            sampled_at_ms: tick,
            emergency_stop_clear: Some(true),
            enclosure_closed: Some(enclosure),
            water_ready: Some(true),
            abrasive_ready: Some(true),
            motion_ready: Some(true),
            pressure_released: Some(released),
        };
        let state = controller.step(tick, inputs, command);
        println!(
            "t={tick} event={command:?} state={state:?} requests={:?}",
            controller.requests()
        );
    }
}
