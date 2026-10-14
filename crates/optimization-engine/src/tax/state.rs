//! State income tax for a single filer (Owl `tax_state.py` and
//! `data/taxes_state.toml`, 2026 basis).
//!
//! Like Owl, every dollar amount is indexed by the plan's inflation, capital
//! gains are taxed as ordinary income, and the state starts from federal AGI
//! less its own standard deduction and retirement-income exemption.

use serde::{Deserialize, Serialize};

/// One state's rules for a single filer.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StateTax {
    /// Two-letter code, for reporting.
    pub code: String,
    /// `(lower bound, rate)` per bracket, lowest first; rates as decimals.
    pub brackets: Vec<(f64, f64)>,
    /// Standard deduction plus personal exemption (2026 dollars).
    pub standard_deduction: f64,
    /// Yearly cap on the exemption for tax-deferred withdrawals and Roth
    /// conversions (2026 dollars); 0 for none.
    pub retirement_exemption: f64,
    /// Age (reached by December 31) from which the exemption applies.
    pub exemption_age: u32,
    /// Whether the state taxes Social Security benefits.
    pub taxes_social_security: bool,
    /// Whether Roth conversions count toward the retirement exemption.
    pub conversions_exempt: bool,
}

impl StateTax {
    /// Virginia (Owl `VA_Single`): standard deduction $8,750 plus the $930
    /// personal exemption, the $12,000 age deduction from 65, no tax on Social
    /// Security.
    pub fn virginia() -> Self {
        Self {
            code: "VA".into(),
            brackets: vec![
                (0.0, 0.02),
                (3_000.0, 0.03),
                (5_000.0, 0.05),
                (17_000.0, 0.0575),
            ],
            standard_deduction: 9_680.0,
            retirement_exemption: 12_000.0,
            exemption_age: 65,
            taxes_social_security: false,
            conversions_exempt: true,
        }
    }

    /// The rules for a two-letter state code, when the engine has them.
    pub fn for_code(code: &str) -> Option<Self> {
        match code.to_ascii_uppercase().as_str() {
            "VA" => Some(Self::virginia()),
            _ => None,
        }
    }

    /// Bracket widths in a year with inflation factor `gamma` (last unbounded).
    pub fn bracket_widths(&self, gamma: f64) -> Vec<f64> {
        self.brackets
            .iter()
            .enumerate()
            .map(|(t, (lower, _))| match self.brackets.get(t + 1) {
                Some((next, _)) => (next - lower) * gamma,
                None => f64::INFINITY,
            })
            .collect()
    }

    /// Tax on state taxable income (after deduction and exemption).
    pub fn tax(&self, taxable_income: f64, gamma: f64) -> f64 {
        let mut remaining = taxable_income.max(0.0);
        let mut tax = 0.0;
        for (width, (_, rate)) in self.bracket_widths(gamma).iter().zip(&self.brackets) {
            let portion = remaining.min(*width);
            tax += portion * rate;
            remaining -= portion;
            if remaining <= 0.0 {
                break;
            }
        }
        tax
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn virginia_brackets() {
        let va = StateTax::virginia();
        // 2% of 3k + 3% of 2k + 5% of 12k + 5.75% of 3k.
        let expected = 60.0 + 60.0 + 600.0 + 172.5;
        assert!((va.tax(20_000.0, 1.0) - expected).abs() < 1e-9);
        assert!((va.tax(0.0, 1.0)).abs() < 1e-12);
        // Brackets are indexed.
        assert!((va.tax(20_000.0 * 1.1, 1.1) - expected * 1.1).abs() < 1e-6);
        assert_eq!(StateTax::for_code("va"), Some(va));
        assert_eq!(StateTax::for_code("TX"), None);
    }
}
