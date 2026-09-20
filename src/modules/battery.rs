use std::marker::PhantomData;

use battery::units::{energy::watt_hour, ratio::percent};
use battery::{Manager, State};

use crate::colors::Color;
use crate::themes::DefaultColors;
use crate::{Powerline, Style};

use super::Module;

/// Battery status is intentionally shown only when the aggregate charge is
/// low enough to need attention. This keeps the prompt quiet for the common
/// case while still making the low-battery warning hard to miss.
const DISPLAY_THRESHOLD_PERCENT: f32 = 10.0;

const FULL_SYMBOL: &str = "\u{f0079}";
const CHARGING_SYMBOL: &str = "\u{f0084}";
const DISCHARGING_SYMBOL: &str = "\u{f0083}";
const UNKNOWN_SYMBOL: &str = "\u{f0091}";
const EMPTY_SYMBOL: &str = "\u{f008e}";

pub struct Battery<S> {
    status: Option<BatteryStatus>,
    scheme: PhantomData<S>,
}

/// Colours for the low-battery warning. Themes can override these through the
/// same per-module colour mechanism as the other widgets.
pub trait BatteryScheme: DefaultColors {
    fn battery_fg() -> Color {
        Self::alert_fg()
    }

    fn battery_bg() -> Color {
        Self::alert_bg()
    }
}

impl<S: BatteryScheme> Default for Battery<S> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S: BatteryScheme> Battery<S> {
    pub fn new() -> Self {
        Self {
            status: battery_status().filter(should_display),
            scheme: PhantomData,
        }
    }
}

impl<S: BatteryScheme> Module for Battery<S> {
    fn append_segments(&mut self, powerline: &mut Powerline) {
        if let Some(status) = self.status {
            powerline.add_segment(
                status.label(),
                Style::simple(S::battery_fg(), S::battery_bg()),
            );
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct BatteryStatus {
    percentage: f32,
    state: State,
}

#[derive(Debug, Clone, Copy)]
struct BatteryReading {
    energy: f32,
    energy_full: f32,
    state: State,
}

impl BatteryStatus {
    fn label(self) -> String {
        format!(
            "{} {}%",
            symbol_for_state(self.state),
            self.percentage.round().clamp(0.0, 100.0) as u8
        )
    }
}

fn symbol_for_state(state: State) -> &'static str {
    match state {
        State::Full => FULL_SYMBOL,
        State::Charging => CHARGING_SYMBOL,
        State::Discharging => DISCHARGING_SYMBOL,
        State::Unknown => UNKNOWN_SYMBOL,
        State::Empty => EMPTY_SYMBOL,
    }
}

fn should_display(status: &BatteryStatus) -> bool {
    status.percentage.is_finite() && status.percentage <= DISPLAY_THRESHOLD_PERCENT
}

/// Read and combine all batteries reported by the operating system. Energy is
/// used as the weight so a small accessory battery cannot skew a laptop's
/// aggregate percentage. A malformed or zero-capacity battery is ignored.
fn battery_status() -> Option<BatteryStatus> {
    let manager = Manager::new().ok()?;
    let batteries = manager.batteries().ok()?;

    let readings = batteries.filter_map(|entry| {
        let battery = entry.ok()?;
        let energy_full = battery.energy_full().get::<watt_hour>();
        let charge_percent = battery.state_of_charge().get::<percent>();

        Some(BatteryReading {
            energy: energy_full * charge_percent / 100.0,
            energy_full,
            state: battery.state(),
        })
    });

    aggregate_readings(readings)
}

fn aggregate_readings(readings: impl IntoIterator<Item = BatteryReading>) -> Option<BatteryStatus> {
    let mut energy = 0.0;
    let mut energy_full = 0.0;
    let mut state = State::Unknown;

    for reading in readings {
        if !reading.energy.is_finite()
            || !reading.energy_full.is_finite()
            || reading.energy_full <= 0.0
        {
            continue;
        }

        energy += reading.energy.clamp(0.0, reading.energy_full);
        energy_full += reading.energy_full;
        state = merge_states(state, reading.state);
    }

    (energy_full > 0.0).then(|| BatteryStatus {
        percentage: (energy / energy_full * 100.0).clamp(0.0, 100.0),
        state,
    })
}

/// Charging wins over discharging when multiple batteries are present. This
/// matches the useful user-facing meaning of the aggregate status: if any
/// battery is actively charging, show the charging indicator.
fn merge_states(first: State, second: State) -> State {
    use State::{Charging, Discharging, Unknown};

    if first == Charging || second == Charging {
        Charging
    } else if first == Discharging || second == Discharging {
        Discharging
    } else if first == second {
        first
    } else if first == Unknown {
        second
    } else if second == Unknown {
        first
    } else {
        Unknown
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reading(energy: f32, energy_full: f32, state: State) -> BatteryReading {
        BatteryReading {
            energy,
            energy_full,
            state,
        }
    }

    #[test]
    fn aggregates_batteries_by_full_energy() {
        let status = aggregate_readings([
            reading(5.0, 10.0, State::Discharging),
            reading(10.0, 20.0, State::Discharging),
        ])
        .expect("valid batteries should produce a status");

        assert_eq!(status.percentage, 50.0);
        assert_eq!(status.state, State::Discharging);
    }

    #[test]
    fn ignores_zero_capacity_and_non_finite_batteries() {
        let status = aggregate_readings([
            reading(0.0, 0.0, State::Full),
            reading(f32::NAN, 10.0, State::Charging),
            reading(2.0, 10.0, State::Discharging),
        ])
        .expect("the valid battery should still be used");

        assert_eq!(status.percentage, 20.0);
        assert_eq!(status.state, State::Discharging);
    }

    #[test]
    fn clamps_invalid_charge_values() {
        let status = aggregate_readings([
            reading(-1.0, 10.0, State::Discharging),
            reading(20.0, 10.0, State::Discharging),
        ])
        .expect("valid capacities should produce a status");

        assert_eq!(status.percentage, 50.0);
    }

    #[test]
    fn empty_readings_have_no_status() {
        assert_eq!(aggregate_readings([]), None);
    }

    #[test]
    fn low_battery_display_requires_finite_percentage_at_or_below_threshold() {
        let at_threshold = BatteryStatus {
            percentage: DISPLAY_THRESHOLD_PERCENT,
            state: State::Discharging,
        };
        let above_threshold = BatteryStatus {
            percentage: DISPLAY_THRESHOLD_PERCENT + 0.01,
            state: State::Discharging,
        };
        let non_finite = BatteryStatus {
            percentage: f32::NAN,
            state: State::Discharging,
        };

        assert!(should_display(&at_threshold));
        assert!(!should_display(&above_threshold));
        assert!(!should_display(&non_finite));
    }

    #[test]
    fn rounds_percentage_in_label() {
        let status = BatteryStatus {
            percentage: 9.6,
            state: State::Discharging,
        };

        assert_eq!(status.label(), format!("{DISCHARGING_SYMBOL} 10%"));
    }

    #[test]
    fn uses_state_specific_symbols() {
        for (state, symbol) in [
            (State::Full, FULL_SYMBOL),
            (State::Charging, CHARGING_SYMBOL),
            (State::Discharging, DISCHARGING_SYMBOL),
            (State::Unknown, UNKNOWN_SYMBOL),
            (State::Empty, EMPTY_SYMBOL),
        ] {
            let status = BatteryStatus {
                percentage: 5.0,
                state,
            };
            assert_eq!(status.label(), format!("{symbol} 5%"));
        }
    }

    #[test]
    fn charging_state_wins_when_batteries_disagree() {
        assert_eq!(
            merge_states(State::Discharging, State::Charging),
            State::Charging
        );
        assert_eq!(merge_states(State::Unknown, State::Empty), State::Empty);
        assert_eq!(merge_states(State::Empty, State::Full), State::Unknown);
    }
}
