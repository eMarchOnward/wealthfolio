"""Regenerate owl_reference.json: solves each reference case with Owl
(https://github.com/mdlacasse/Owl) so the Rust engine can be checked against it.

Run with Owl's own interpreter, e.g.:
    C:\\Users\\ebone\\scripts\\Owl\\.venv\\Scripts\\python.exe run_owl.py

Every case mirrors a scenario built in tests/owl_parity.rs. Owl is configured
to the engine's scope: one filer, no state tax, no bracket sunset, flat spending, fixed rates with
a 100% equity allocation (so every account earns the same return), loop-mode
LTCG/NIIT and exact (MILP) Medicare.
"""

import json
from pathlib import Path

import numpy as np
import pandas as pd
import owlplanner as owl

START_YEAR = 2026
HERE = Path(__file__).resolve().parent


def build(case):
    plan = owl.Plan(["Pat"], [case["dob"]], [case["planning_age"]], case["name"], verbose=False)
    plan.setSexes([case.get("sex", "M")])
    plan.setAccountBalances(
        taxable=[case["taxable"] / 1000],
        taxDeferred=[case["tax_deferred"] / 1000],
        taxFree=[case["roth"] / 1000],
        startDate="01-01",
    )
    if case["taxable"] > 0 and case["taxable_cost_basis"] < case["taxable"]:
        plan.setCostBasis([case["taxable_cost_basis"] / 1000])
    plan.setSocialSecurity([case["pia"]], [case["claim_age"]])
    plan.setRates("user", values=[case["return"] * 100, 0, 0, case["inflation"] * 100])
    plan.setAllocationRatios("individual", generic=[[[100, 0, 0, 0], [100, 0, 0, 0]]])
    plan.setDividendRate(case["dividend_yield"] * 100)
    plan.setHeirsTaxRate(30)
    plan.setSpendingProfile("flat")
    # OBBBA made the current brackets permanent; Owl defaults to a 2032 sunset.
    plan.setExpirationYearOBBBA(2100)

    years = list(range(START_YEAR - 5, START_YEAR + case["planning_age"] - (START_YEAR - int(case["dob"][:4])) + 1))
    table = pd.DataFrame({"year": years})
    for column in ("anticipated wages", "401k ctrb", "Roth 401k ctrb", "taxable ctrb"):
        table[column] = 0.0
    for year, row in case.get("working", {}).items():
        index = years.index(int(year))
        for column, amount in row.items():
            table.loc[index, column] = amount
    plan.readHFP({"Pat": table}, filename_for_logging=case["name"])
    return plan


def solve(case):
    plan = build(case)
    options = {
        "previousMAGIs": [m / 1000 for m in case["prior_magi"]],
        "withMedicare": "optimize",
        "maxIter": 29,
    }
    if case["objective"] == "maxSpending":
        options["bequest"] = case["bequest_floor"] / 1000
    else:
        options["netSpending"] = case["net_spending"] / 1000
    plan.solve(case["objective"], options=options)
    if plan.caseStatus != "solved":
        raise RuntimeError(f"{case['name']}: {plan.caseStatus}")

    gamma = plan.gamma_n[: plan.N_n]
    return {
        "first_year_spending_today": float(plan.g_n[0] / gamma[0]),
        "bequest_after_tax_today": float(plan.bequest),
        "spending_today": [float(x) for x in plan.g_n / gamma],
        "tax_deferred_withdrawal": [float(x) for x in plan.w_ijn[0, 1, :]],
        "taxable_withdrawal": [float(x) for x in plan.w_ijn[0, 0, :]],
        "roth_withdrawal": [float(x) for x in plan.w_ijn[0, 2, :]],
        "roth_conversion": [float(x) for x in plan.x_in[0, :]],
        "ordinary_tax": [float(x) for x in plan.T_n],
        "ltcg_tax": [float(x) for x in plan.U_n],
        "niit": [float(x) for x in plan.J_n],
        "medicare": [float(x) for x in plan.m_n + plan.M_n],
        "magi": [float(x) for x in plan.MAGI_n],
        "ordinary_income": [float(x) for x in plan.G_n],
        "tax_deferred_balance": [float(x) for x in plan.b_ijn[0, 1, : plan.N_n]],
        "roth_balance": [float(x) for x in plan.b_ijn[0, 2, : plan.N_n]],
        # Owl's own disagreement between modelled and statutory LTCG tax.
        "ltcg_residual": float(plan.fixedPointResidual["LTCG"]["abs_sum"]),
    }


STANDARD_RETIREE = {
    "dob": "1964-06-15",
    "planning_age": 95,
    "taxable": 500_000.0,
    "taxable_cost_basis": 350_000.0,
    "tax_deferred": 1_000_000.0,
    "roth": 0.0,
    "pia": 2_800.0,
    "claim_age": 67.0,
    "return": 0.05,
    "inflation": 0.025,
    "dividend_yield": 0.015,
    "prior_magi": [90_000.0, 90_000.0],
}

# A worker who stops at 55: wages are taxable W-2 income (already net of the
# pre-tax 401(k) deferral, which is Owl's convention for that column).
EARLY_RETIREE = {
    "dob": "1981-04-15",
    "planning_age": 92,
    "taxable": 250_000.0,
    "taxable_cost_basis": 180_000.0,
    "tax_deferred": 400_000.0,
    "roth": 60_000.0,
    "pia": 2_400.0,
    "claim_age": 67.0,
    "return": 0.05,
    "inflation": 0.025,
    "dividend_yield": 0.015,
    "prior_magi": [150_000.0, 150_000.0],
    "working": {
        str(year): {"anticipated wages": 140_000.0 * 1.025 ** (year - START_YEAR),
                    "401k ctrb": 24_500.0 * 1.025 ** (year - START_YEAR)}
        for year in range(START_YEAR, START_YEAR + 10)
    },
}

# Owl's self-consistent LTCG loop can settle on a plan that under-taxes gains
# (its own fixedPointResidual reports it), so strict parity cases hold no
# taxable account: its balance moves to the Roth account, LTCG never arises
# and Owl's answer is exact.
def no_gains(case):
    return {**case, "roth": case["roth"] + case["taxable"], "taxable": 0.0,
            "taxable_cost_basis": 0.0, "dividend_yield": 0.0}


CASES = {
    "standard_retiree_max_spending": {**STANDARD_RETIREE, "objective": "maxSpending", "bequest_floor": 0.0},
    "standard_retiree_max_bequest": {**STANDARD_RETIREE, "objective": "maxBequest", "net_spending": 80_000.0},
    "standard_retiree_no_gains_max_spending": {
        **no_gains(STANDARD_RETIREE), "objective": "maxSpending", "bequest_floor": 0.0},
    "standard_retiree_no_gains_max_bequest": {
        **no_gains(STANDARD_RETIREE), "objective": "maxBequest", "net_spending": 80_000.0},
    "early_retiree_no_gains_max_spending": {
        **no_gains(EARLY_RETIREE), "objective": "maxSpending", "bequest_floor": 0.0},
    "early_retiree_no_gains_max_bequest": {
        **no_gains(EARLY_RETIREE), "objective": "maxBequest", "net_spending": 70_000.0},
    "early_retiree_no_gains_no_ss_max_bequest": {
        **no_gains(EARLY_RETIREE), "pia": 0.0, "objective": "maxBequest", "net_spending": 55_000.0},
}


def main():
    results = {}
    for name, case in CASES.items():
        case = {**case, "name": name}
        print(f"solving {name} ...")
        results[name] = solve(case)
        print(f"  first-year spending {results[name]['first_year_spending_today']:,.0f}"
              f"  bequest {results[name]['bequest_after_tax_today']:,.0f}")
    (HERE / "owl_reference.json").write_text(json.dumps(results, indent=1))


if __name__ == "__main__":
    main()
