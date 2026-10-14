//! ACA marketplace premiums after the Premium Tax Credit for a single filer
//! (Owl `tax_federal.acaCosts`, 2026 rules: Rev. Proc. 2025-25, no credit above
//! 400% of the federal poverty level).

/// 2026 federal poverty level for a household of one (48 states + DC).
pub const FPL_2026_SINGLE: f64 = 15_960.0;

/// Income (as a multiple of FPL) where each contribution percentage applies.
const BREAKPOINTS: [f64; 6] = [1.33, 1.50, 2.00, 2.50, 3.00, 4.00];
/// Required contribution (share of MAGI) at each breakpoint, interpolated between.
const CONTRIBUTION: [f64; 6] = [0.021, 0.0419, 0.066, 0.0844, 0.0996, 0.0996];
/// Below this multiple of FPL the person is Medicaid-eligible; like Owl, the
/// marketplace cost there is the full benchmark premium.
const MEDICAID_LIMIT: f64 = 1.38;
/// Above this multiple of FPL there is no credit (the 2026 "cliff").
pub const CLIFF: f64 = 4.0;

fn contribution_share(ratio: f64) -> f64 {
    if ratio < BREAKPOINTS[0] {
        return CONTRIBUTION[0];
    }
    for i in 0..BREAKPOINTS.len() - 1 {
        let (lo, hi) = (BREAKPOINTS[i], BREAKPOINTS[i + 1]);
        if ratio < hi {
            let w = (ratio - lo) / (hi - lo);
            return CONTRIBUTION[i] + w * (CONTRIBUTION[i + 1] - CONTRIBUTION[i]);
        }
    }
    CONTRIBUTION[BREAKPOINTS.len() - 1]
}

/// Net yearly premium for the benchmark (second-lowest silver) plan costing
/// `slcsp`, given `magi`, both nominal; `fpl` is that year's poverty level.
pub fn net_premium(magi: f64, slcsp: f64, fpl: f64) -> f64 {
    if slcsp <= 0.0 {
        return 0.0;
    }
    let ratio = magi / fpl;
    if !(MEDICAID_LIMIT..CLIFF).contains(&ratio) {
        return slcsp;
    }
    slcsp.min(contribution_share(ratio) * magi)
}

/// The premium just under the cliff: the most a subsidized year can cost.
pub fn premium_below_cliff(slcsp: f64, fpl: f64) -> f64 {
    slcsp.min(CONTRIBUTION[BREAKPOINTS.len() - 1] * CLIFF * fpl)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn credit_follows_the_2026_schedule() {
        let fpl = FPL_2026_SINGLE;
        let slcsp = 9_000.0;
        // 200% FPL: 6.6% of MAGI.
        assert!((net_premium(2.0 * fpl, slcsp, fpl) - 0.066 * 2.0 * fpl).abs() < 1e-6);
        // Between breakpoints the share is interpolated.
        let mid = net_premium(2.25 * fpl, slcsp, fpl);
        assert!((mid - (0.066 + 0.5 * (0.0844 - 0.066)) * 2.25 * fpl).abs() < 1e-6);
        // Above the cliff and in Medicaid territory: the full premium.
        assert_eq!(net_premium(4.01 * fpl, slcsp, fpl), slcsp);
        assert_eq!(net_premium(1.0 * fpl, slcsp, fpl), slcsp);
        // The subsidized cost never exceeds the benchmark.
        assert_eq!(net_premium(3.9 * fpl, 1_000.0, fpl), 1_000.0);
        assert!(premium_below_cliff(slcsp, fpl) <= slcsp);
    }
}
