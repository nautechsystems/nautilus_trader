// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! Command-line arguments shared by every book stress harness.

use std::{
    fmt::{Display, Write},
    str::FromStr,
};

use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;

/// Parsed `--scenario`, `--timeout`, and `--rounds` arguments plus the venue's own flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct StressArgs {
    scenario: String,
    timeout_secs: u64,
    rounds: usize,
    flags: Vec<(&'static str, String)>,
}

impl StressArgs {
    /// Parses `args` as `--name value` pairs.
    ///
    /// The first of `scenarios` is the default scenario, and `rounds` is the default round count.
    ///
    /// # Errors
    ///
    /// Returns a message naming the first positional argument, unknown flag or scenario, missing
    /// value, or invalid number.
    pub(crate) fn parse(
        args: impl IntoIterator<Item = String>,
        scenarios: &[&str],
        rounds: usize,
        flags: &[Flag],
    ) -> Result<Self, String> {
        let mut parsed = Self {
            scenario: scenarios[0].to_string(),
            timeout_secs: DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS,
            rounds,
            flags: flags
                .iter()
                .map(|flag| (flag.name, flag.default.to_string()))
                .collect(),
        };

        let mut args = args.into_iter();

        while let Some(arg) = args.next() {
            let Some(name) = arg.strip_prefix("--") else {
                return Err(format!("unexpected argument {arg}"));
            };

            let Some(value) = args.next() else {
                return Err(format!("missing value for --{name}"));
            };

            match name {
                "scenario" if scenarios.contains(&value.as_str()) => parsed.scenario = value,
                "scenario" => return Err(format!("unknown scenario {value}")),
                "timeout" => parsed.timeout_secs = parse_number(name, &value)?,
                "rounds" => parsed.rounds = parse_number(name, &value)?,
                _ => {
                    let Some((_, slot)) = parsed.flags.iter_mut().find(|(flag, _)| *flag == name)
                    else {
                        return Err(format!("unknown flag --{name}"));
                    };

                    *slot = value;
                }
            }
        }

        Ok(parsed)
    }

    /// Returns the scenario name.
    #[must_use]
    pub(crate) fn scenario(&self) -> &str {
        &self.scenario
    }

    /// Returns the snapshot timeout in seconds, where zero disables snapshot deadlines.
    #[must_use]
    pub(crate) const fn timeout_secs(&self) -> u64 {
        self.timeout_secs
    }

    /// Returns the number of stress rounds.
    #[must_use]
    pub(crate) const fn rounds(&self) -> usize {
        self.rounds
    }

    /// Returns the value of a venue flag.
    ///
    /// # Panics
    ///
    /// Panics if `name` is not a declared venue flag.
    #[must_use]
    pub(crate) fn flag(&self, name: &str) -> &str {
        self.flags
            .iter()
            .find(|(flag, _)| *flag == name)
            .map_or_else(
                || panic!("--{name} is not a declared flag"),
                |(_, value)| value.as_str(),
            )
    }
}

impl Display for StressArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "scenario={} timeout={} rounds={}",
            self.scenario, self.timeout_secs, self.rounds
        )?;

        for (name, value) in &self.flags {
            write!(f, " {name}={value}")?;
        }

        Ok(())
    }
}

/// A venue flag and the value used when the flag is absent.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Flag {
    /// Flag name without the leading `--`.
    pub(crate) name: &'static str,
    /// Value used when the flag is absent.
    pub(crate) default: &'static str,
    /// Description for the usage text.
    pub(crate) help: &'static str,
}

pub(super) fn usage(venue: &str, scenarios: &[&str], rounds: usize, flags: &[Flag]) -> String {
    let mut usage =
        format!("Usage: {venue}-book-stress [--scenario NAME] [--timeout SECS] [--rounds N]");

    for flag in flags {
        let _ = write!(usage, " [--{} VALUE]", flag.name);
    }

    let _ = write!(
        usage,
        "\n\n--scenario  {} (default {})\n\
         --timeout   Snapshot timeout in seconds, 0 disables deadlines (default \
         {DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS})\n\
         --rounds    Stress rounds (default {rounds})",
        scenarios.join(", "),
        scenarios[0],
    );

    for flag in flags {
        let _ = write!(
            usage,
            "\n--{:<10}{} (default {})",
            flag.name, flag.help, flag.default
        );
    }

    usage
}

fn parse_number<T: FromStr>(name: &str, value: &str) -> Result<T, String> {
    value
        .parse()
        .map_err(|_| format!("invalid value {value} for --{name}"))
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const SCENARIOS: [&str; 3] = ["churn", "boundaries", "quiet"];
    const FLAGS: [Flag; 2] = [
        Flag {
            name: "product",
            default: "spot",
            help: "Product",
        },
        Flag {
            name: "books",
            default: "4",
            help: "Book count",
        },
    ];

    fn parse(args: &[&str]) -> Result<StressArgs, String> {
        StressArgs::parse(args.iter().map(ToString::to_string), &SCENARIOS, 14, &FLAGS)
    }

    #[rstest]
    fn defaults_apply_without_arguments() {
        let args = parse(&[]).unwrap();

        assert_eq!(args.scenario(), "churn");
        assert_eq!(args.timeout_secs(), DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS);
        assert_eq!(args.rounds(), 14);
        assert_eq!(args.flag("product"), "spot");
        assert_eq!(args.flag("books"), "4");
    }

    #[rstest]
    fn flags_override_defaults() {
        let args = parse(&[
            "--books",
            "12",
            "--scenario",
            "quiet",
            "--timeout",
            "0",
            "--rounds",
            "3",
            "--product",
            "futures",
        ])
        .unwrap();

        assert_eq!(args.scenario(), "quiet");
        assert_eq!(args.timeout_secs(), 0);
        assert_eq!(args.rounds(), 3);
        assert_eq!(args.flag("product"), "futures");
        assert_eq!(args.flag("books"), "12");
        assert_eq!(
            args.to_string(),
            "scenario=quiet timeout=0 rounds=3 product=futures books=12"
        );
    }

    #[rstest]
    #[case::positional(&["10", "18"], "unexpected argument 10")]
    #[case::unknown_flag(&["--count", "4"], "unknown flag --count")]
    #[case::unknown_scenario(&["--scenario", "turnover"], "unknown scenario turnover")]
    #[case::missing_value(&["--rounds"], "missing value for --rounds")]
    #[case::invalid_timeout(&["--timeout", "-1"], "invalid value -1 for --timeout")]
    #[case::invalid_rounds(&["--rounds", "many"], "invalid value many for --rounds")]
    fn invalid_arguments_fail(#[case] args: &[&str], #[case] expected: &str) {
        assert_eq!(parse(args), Err(expected.to_string()));
    }

    #[rstest]
    fn usage_lists_scenarios_and_flags() {
        let usage = usage("binance", &SCENARIOS, 14, &FLAGS);

        assert!(usage.starts_with(
            "Usage: binance-book-stress [--scenario NAME] [--timeout SECS] [--rounds N] \
             [--product VALUE] [--books VALUE]"
        ));
        assert!(usage.contains("--scenario  churn, boundaries, quiet (default churn)"));
        assert!(usage.contains("--rounds    Stress rounds (default 14)"));
        assert!(usage.contains("--product   Product (default spot)"));
        assert!(usage.contains("--books     Book count (default 4)"));
    }
}
