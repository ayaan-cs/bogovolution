//! The fixed public rule that turns benchmark runs into a verdict.
//!
//! The rule is a pure function so anyone can read it, test it, and trust that
//! nothing else (no model, no judgement call) decides what is kept.
//!
//! Inputs: the pass rate of each run for the parent and for the child. Run
//! `i` of the parent is paired with run `i` of the child.
//!
//! * `parent_median`, `child_median`: the median pass rate (for an even count,
//!   the mean of the two middle values).
//! * `gain_pp` = (child_median - parent_median) * 100, in percentage points.
//! * `paired_wins`: how many pairs the child won. A win means the child's pass
//!   rate is strictly higher. A tie is not a win.
//!
//! Verdict:
//! 1. `accepted` if `gain_pp >= accept_threshold_pp` AND
//!    `paired_wins >= min_paired_wins`.
//! 2. else `rejected` if `child_median <= parent_median`.
//! 3. else `noise` (the child looks a little better, but not clearly).
//!
//! Held out gate (v2.3.0). When the task set has held out tasks, `passed`
//! and the rates above count train tasks only, and held out tasks are scored
//! the same way on their own (held out medians, gain and paired wins over the
//! same paired runs). A child that rule 1 accepts is then kept only if also
//! `held_out_gain_pp >= held_out_accept_threshold_pp` (default 10) AND
//! `held_out_paired_wins >= held_out_min_paired_wins` (default 3 of 5).
//! Otherwise the verdict is `overfit`, and `current` does not move. Rejected
//! and noise are decided by the train rule alone, as before. A task set with
//! no held out tasks behaves exactly as before.

use crate::event::VerdictKind;
use serde::Serialize;

/// Tolerance for comparing floating point rates. Rates come from small
/// fractions like 3/5, so this only guards against rounding such as
/// `0.6 - 0.5 = 0.0999999999`.
const EPS: f64 = 1e-9;

/// The numbers the rule uses. Taken from `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rule {
    pub accept_threshold_pp: f64,
    pub min_paired_wins: u32,
}

/// The held out gate. Taken from `config.toml`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeldOutRule {
    pub accept_threshold_pp: f64,
    pub min_paired_wins: u32,
}

/// How the child did on the held out tasks.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HeldOutDecision {
    pub parent_median: f64,
    pub child_median: f64,
    pub gain_pp: f64,
    pub paired_wins: u32,
    pub pairs: Vec<Pair>,
    /// Whether the gate is met.
    pub passed: bool,
}

/// Is a child's pass rate a strict win over its parent's? Same test as the
/// rule uses for paired wins.
pub fn pair_win(parent_rate: f64, child_rate: f64) -> bool {
    child_rate > parent_rate + EPS
}

/// One paired comparison.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Pair {
    pub run_index: u32,
    pub parent_rate: f64,
    pub child_rate: f64,
    pub win: bool,
}

/// The result of applying the rule.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Decision {
    pub verdict: VerdictKind,
    pub parent_median: f64,
    pub child_median: f64,
    pub gain_pp: f64,
    pub paired_wins: u32,
    pub repeats: u32,
    pub pairs: Vec<Pair>,
    /// Plain English explanation, also stored in the log.
    pub reason: String,
    /// The held out gate, when the task set has held out tasks.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub held_out: Option<HeldOutDecision>,
}

/// Why the rule could not be applied.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum RuleError {
    #[error("parent and child need the same number of runs (got {parent} and {child})")]
    LengthMismatch { parent: usize, child: usize },
    #[error("there are no runs to compare")]
    NoRuns,
    #[error("pass rates must be numbers from 0 to 1 (got {0})")]
    BadRate(f64),
}

/// Median of a non-empty list. Even counts use the mean of the middle two.
pub fn median(values: &[f64]) -> f64 {
    let mut v = values.to_vec();
    v.sort_by(|a, b| a.partial_cmp(b).expect("rates are not NaN"));
    let n = v.len();
    if n % 2 == 1 {
        v[n / 2]
    } else {
        (v[n / 2 - 1] + v[n / 2]) / 2.0
    }
}

/// Apply the rule. `parent_rates[i]` and `child_rates[i]` are the pass rates
/// of paired run `i`.
pub fn decide(
    parent_rates: &[f64],
    child_rates: &[f64],
    rule: &Rule,
) -> Result<Decision, RuleError> {
    if parent_rates.len() != child_rates.len() {
        return Err(RuleError::LengthMismatch {
            parent: parent_rates.len(),
            child: child_rates.len(),
        });
    }
    if parent_rates.is_empty() {
        return Err(RuleError::NoRuns);
    }
    for &r in parent_rates.iter().chain(child_rates.iter()) {
        if !(0.0..=1.0).contains(&r) || r.is_nan() {
            return Err(RuleError::BadRate(r));
        }
    }

    let pairs: Vec<Pair> = parent_rates
        .iter()
        .zip(child_rates.iter())
        .enumerate()
        .map(|(i, (&p, &c))| Pair {
            run_index: i as u32,
            parent_rate: p,
            child_rate: c,
            win: pair_win(p, c),
        })
        .collect();
    let paired_wins = pairs.iter().filter(|p| p.win).count() as u32;
    let parent_median = median(parent_rates);
    let child_median = median(child_rates);
    let gain_pp = (child_median - parent_median) * 100.0;
    let repeats = pairs.len() as u32;

    let gain_ok = gain_pp + EPS >= rule.accept_threshold_pp;
    let wins_ok = paired_wins >= rule.min_paired_wins;
    let (verdict, reason) = if gain_ok && wins_ok {
        (
            VerdictKind::Accepted,
            format!(
                "median gain {gain_pp:.1} points is at least {} and the child won {paired_wins} of {repeats} pairs (needs {})",
                rule.accept_threshold_pp, rule.min_paired_wins
            ),
        )
    } else if child_median <= parent_median + EPS {
        (
            VerdictKind::Rejected,
            format!("child median {child_median:.3} is not above parent median {parent_median:.3}"),
        )
    } else {
        let mut why = Vec::new();
        if !gain_ok {
            why.push(format!(
                "median gain {gain_pp:.1} points is below {}",
                rule.accept_threshold_pp
            ));
        }
        if !wins_ok {
            why.push(format!(
                "the child won {paired_wins} of {repeats} pairs (needs {})",
                rule.min_paired_wins
            ));
        }
        (VerdictKind::Noise, why.join(" and "))
    };

    Ok(Decision {
        verdict,
        parent_median,
        child_median,
        gain_pp,
        paired_wins,
        repeats,
        pairs,
        reason,
        held_out: None,
    })
}

fn check_rates(parent: &[f64], child: &[f64]) -> Result<Vec<Pair>, RuleError> {
    if parent.len() != child.len() {
        return Err(RuleError::LengthMismatch {
            parent: parent.len(),
            child: child.len(),
        });
    }
    if parent.is_empty() {
        return Err(RuleError::NoRuns);
    }
    for &r in parent.iter().chain(child.iter()) {
        if !(0.0..=1.0).contains(&r) || r.is_nan() {
            return Err(RuleError::BadRate(r));
        }
    }
    Ok(parent
        .iter()
        .zip(child.iter())
        .enumerate()
        .map(|(i, (&p, &c))| Pair {
            run_index: i as u32,
            parent_rate: p,
            child_rate: c,
            win: pair_win(p, c),
        })
        .collect())
}

/// Apply the train rule, then the held out gate. `held_parent[i]` and
/// `held_child[i]` are the held out pass rates of the same paired run `i`.
pub fn decide_with_held_out(
    parent_rates: &[f64],
    child_rates: &[f64],
    rule: &Rule,
    held_parent: &[f64],
    held_child: &[f64],
    gate: &HeldOutRule,
) -> Result<Decision, RuleError> {
    let mut d = decide(parent_rates, child_rates, rule)?;
    let pairs = check_rates(held_parent, held_child)?;
    if pairs.len() != d.pairs.len() {
        return Err(RuleError::LengthMismatch {
            parent: d.pairs.len(),
            child: pairs.len(),
        });
    }
    let paired_wins = pairs.iter().filter(|p| p.win).count() as u32;
    let parent_median = median(held_parent);
    let child_median = median(held_child);
    let gain_pp = (child_median - parent_median) * 100.0;
    let gain_ok = gain_pp + EPS >= gate.accept_threshold_pp;
    let wins_ok = paired_wins >= gate.min_paired_wins;
    let passed = gain_ok && wins_ok;
    let repeats = pairs.len();
    let held_text = format!(
        "held out median gain {gain_pp:.1} points (needs {}), held out wins {paired_wins} of {repeats} (needs {})",
        gate.accept_threshold_pp, gate.min_paired_wins
    );
    if d.verdict == VerdictKind::Accepted {
        if passed {
            d.reason = format!("{}; {held_text}", d.reason);
        } else {
            d.verdict = VerdictKind::Overfit;
            d.reason = format!(
                "the train rule would accept ({}), but on the held out tasks: {held_text}",
                d.reason
            );
        }
    }
    d.held_out = Some(HeldOutDecision {
        parent_median,
        child_median,
        gain_pp,
        paired_wins,
        pairs,
        passed,
    });
    Ok(d)
}

#[cfg(test)]
mod tests {
    use super::*;

    const RULE: Rule = Rule {
        accept_threshold_pp: 10.0,
        min_paired_wins: 4,
    };

    fn v(parent: &[f64], child: &[f64]) -> VerdictKind {
        decide(parent, child, &RULE).unwrap().verdict
    }

    const GATE: HeldOutRule = HeldOutRule {
        accept_threshold_pp: 10.0,
        min_paired_wins: 3,
    };

    fn h(train: (&[f64], &[f64]), held: (&[f64], &[f64])) -> Decision {
        decide_with_held_out(train.0, train.1, &RULE, held.0, held.1, &GATE).unwrap()
    }

    #[test]
    fn held_out_gate_needs_a_gain_on_both() {
        let win = (&[0.5; 5][..], &[0.75; 5][..]);
        // gain on both: accepted
        let d = h(win, (&[0.4; 5], &[0.6; 5]));
        assert_eq!(d.verdict, VerdictKind::Accepted);
        let ho = d.held_out.as_ref().unwrap();
        assert!(ho.passed && ho.paired_wins == 5 && (ho.gain_pp - 20.0).abs() < 1e-9);
        assert!(
            d.reason.contains("held out median gain 20.0"),
            "{}",
            d.reason
        );
        // flat held out: overfit, with a reason that says why
        let d = h(win, (&[0.5; 5], &[0.5; 5]));
        assert_eq!(d.verdict, VerdictKind::Overfit);
        assert!(d.reason.contains("train rule would accept"), "{}", d.reason);
        // worse held out: overfit
        assert_eq!(h(win, (&[0.5; 5], &[0.2; 5])).verdict, VerdictKind::Overfit);
        // held out gain but only 2 wins: overfit
        let d = h(
            win,
            (&[0.5, 0.5, 0.5, 0.9, 0.9], &[0.9, 0.9, 0.5, 0.5, 0.9]),
        );
        assert_eq!(d.held_out.as_ref().unwrap().paired_wins, 2);
        assert_eq!(d.verdict, VerdictKind::Overfit);
        // exactly 10 points and 3 wins passes
        let d = h(win, (&[0.5; 5], &[0.6, 0.6, 0.6, 0.5, 0.5]));
        assert_eq!(d.verdict, VerdictKind::Accepted, "{}", d.reason);
    }

    #[test]
    fn held_out_never_rescues_or_changes_a_train_reject_or_noise() {
        let good_held = (&[0.2; 5][..], &[0.9; 5][..]);
        assert_eq!(
            h((&[0.5; 5], &[0.5; 5]), good_held).verdict,
            VerdictKind::Rejected
        );
        assert_eq!(
            h((&[0.5; 5], &[0.55; 5]), good_held).verdict,
            VerdictKind::Noise
        );
        // and the train numbers are the train numbers
        let d = h((&[0.5; 5], &[0.75; 5]), (&[0.5; 5], &[0.5; 5]));
        assert!((d.gain_pp - 25.0).abs() < 1e-9);
        assert!((d.child_median - 0.75).abs() < 1e-9);
    }

    #[test]
    fn held_out_rates_are_checked() {
        let t = [0.5; 5];
        assert!(decide_with_held_out(&t, &t, &RULE, &[0.5; 4], &[0.5; 4], &GATE).is_err());
        assert!(decide_with_held_out(&t, &t, &RULE, &[0.5; 5], &[1.5; 5], &GATE).is_err());
    }

    #[test]
    fn clear_win_is_accepted() {
        let d = decide(&[0.2; 5], &[0.8; 5], &RULE).unwrap();
        assert_eq!(d.verdict, VerdictKind::Accepted);
        assert_eq!(d.paired_wins, 5);
        assert!((d.gain_pp - 60.0).abs() < 1e-9);
        assert_eq!(d.repeats, 5);
    }

    #[test]
    fn exactly_ten_points_and_four_wins_is_accepted() {
        // medians 0.5 and 0.6, which is 0.0999999.. in floating point
        let parent = [0.5, 0.5, 0.5, 0.5, 0.5];
        let child = [0.6, 0.6, 0.6, 0.6, 0.5];
        let d = decide(&parent, &child, &RULE).unwrap();
        assert_eq!(d.paired_wins, 4);
        assert_eq!(d.verdict, VerdictKind::Accepted);
    }

    #[test]
    fn just_under_threshold_is_noise() {
        let parent = [0.5; 5];
        let child = [0.59; 5];
        assert_eq!(v(&parent, &child), VerdictKind::Noise);
    }

    #[test]
    fn big_gain_with_only_three_wins_is_noise() {
        // child median is 0.9 (gain 40) but two pairs are ties or losses
        let parent = [0.5, 0.5, 0.5, 0.9, 0.9];
        let child = [0.9, 0.9, 0.9, 0.9, 0.5];
        let d = decide(&parent, &child, &RULE).unwrap();
        assert_eq!(d.paired_wins, 3);
        assert!(d.gain_pp >= 10.0);
        assert_eq!(d.verdict, VerdictKind::Noise);
    }

    #[test]
    fn equal_medians_are_rejected_even_with_wins() {
        let parent = [0.5, 0.5, 0.5, 0.5, 0.5];
        let child = [0.5, 0.5, 0.5, 0.0, 0.0];
        assert_eq!(v(&parent, &child), VerdictKind::Rejected);
        // Identical runs: no wins, no gain.
        assert_eq!(v(&[0.3; 5], &[0.3; 5]), VerdictKind::Rejected);
    }

    #[test]
    fn lower_child_is_rejected() {
        assert_eq!(v(&[0.8; 5], &[0.2; 5]), VerdictKind::Rejected);
    }

    #[test]
    fn small_positive_gain_is_noise_not_rejected() {
        let parent = [0.5; 5];
        let child = [0.55; 5];
        assert_eq!(v(&parent, &child), VerdictKind::Noise);
    }

    #[test]
    fn ties_are_not_wins() {
        let d = decide(&[0.5; 5], &[0.5; 5], &RULE).unwrap();
        assert_eq!(d.paired_wins, 0);
    }

    #[test]
    fn median_handles_even_counts_and_unsorted_input() {
        assert!((median(&[0.9, 0.1, 0.5]) - 0.5).abs() < 1e-12);
        assert!((median(&[0.2, 0.8, 0.4, 0.6]) - 0.5).abs() < 1e-12);
        assert!((median(&[0.7]) - 0.7).abs() < 1e-12);
    }

    #[test]
    fn median_not_mean_decides() {
        // One huge outlier run does not drag the median.
        let parent = [0.2, 0.2, 0.2, 0.2, 0.2];
        let child = [1.0, 0.2, 0.2, 0.2, 0.2];
        assert_eq!(v(&parent, &child), VerdictKind::Rejected);
    }

    #[test]
    fn other_repeat_counts_work() {
        let rule = Rule {
            accept_threshold_pp: 10.0,
            min_paired_wins: 2,
        };
        let d = decide(&[0.1, 0.1, 0.1], &[0.9, 0.9, 0.1], &rule).unwrap();
        assert_eq!(d.verdict, VerdictKind::Accepted);
        assert_eq!(d.repeats, 3);
        let d = decide(&[0.1, 0.1], &[0.9, 0.9], &rule).unwrap();
        assert_eq!(d.verdict, VerdictKind::Accepted);
    }

    #[test]
    fn bad_input_is_an_error() {
        assert_eq!(
            decide(&[0.1], &[0.1, 0.2], &RULE).unwrap_err(),
            RuleError::LengthMismatch {
                parent: 1,
                child: 2
            }
        );
        assert_eq!(decide(&[], &[], &RULE).unwrap_err(), RuleError::NoRuns);
        assert!(matches!(
            decide(&[1.5], &[0.1], &RULE),
            Err(RuleError::BadRate(_))
        ));
        assert!(matches!(
            decide(&[f64::NAN], &[0.1], &RULE),
            Err(RuleError::BadRate(_))
        ));
    }
}
