//! A planned home purchase, kept in a home goal's savings plan and linkable
//! into the tax-optimized retirement plan.

use chrono::{Datelike, NaiveDate};
use serde::{Deserialize, Serialize};

use crate::errors::{Result, ValidationError};
use crate::planning::retirement::ExpenseBucket;

/// Key of the purchase in a home goal's save-up settings JSON.
pub const HOME_PURCHASE_SETTINGS_KEY: &str = "homePurchase";

/// The home to buy. Money is in today's currency; rates are decimals.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct HomePurchase {
    pub price: f64,
    pub down_payment_rate: f64,
    /// Closing costs as a share of the price.
    pub closing_cost_rate: f64,
    pub mortgage_rate: f64,
    pub term_years: u32,
    /// Yearly property tax as a share of the home's value.
    pub property_tax_rate: f64,
    /// Yearly homeowner's insurance.
    pub insurance: f64,
    /// Yearly maintenance as a share of the home's value.
    pub maintenance_rate: f64,
    pub hoa_monthly: f64,
    /// Yearly growth of the home's price (nominal).
    pub appreciation: f64,
    /// Buy at this age instead of on the goal's target date.
    pub purchase_age: Option<u32>,
}

impl Default for HomePurchase {
    fn default() -> Self {
        Self {
            price: 0.0,
            down_payment_rate: 0.20,
            closing_cost_rate: 0.03,
            mortgage_rate: 0.065,
            term_years: 30,
            property_tax_rate: 0.01,
            insurance: 1_500.0,
            maintenance_rate: 0.01,
            hoa_monthly: 0.0,
            appreciation: 0.03,
            purchase_age: None,
        }
    }
}

impl HomePurchase {
    /// Monthly principal and interest on `principal`.
    pub fn monthly_payment(&self, principal: f64) -> f64 {
        let months = f64::from(self.term_years * 12);
        if principal <= 0.0 || months <= 0.0 {
            return 0.0;
        }
        let r = self.mortgage_rate / 12.0;
        if r <= 0.0 {
            principal / months
        } else {
            principal * r / (1.0 - (1.0 + r).powf(-months))
        }
    }

    pub fn validate(&self) -> Result<()> {
        let bad = |message: &str| -> Result<()> {
            Err(ValidationError::InvalidInput(format!("Home purchase: {message}")).into())
        };
        let finite = [
            self.price,
            self.insurance,
            self.hoa_monthly,
            self.down_payment_rate,
            self.closing_cost_rate,
            self.mortgage_rate,
            self.property_tax_rate,
            self.maintenance_rate,
            self.appreciation,
        ];
        if finite.iter().any(|v| !v.is_finite()) {
            return bad("every amount must be a number");
        }
        if self.price <= 0.0 {
            return bad("enter the home's price");
        }
        if self.insurance < 0.0 || self.hoa_monthly < 0.0 {
            return bad("insurance and HOA dues can't be negative");
        }
        if !(0.0..=1.0).contains(&self.down_payment_rate) {
            return bad("the down payment must be between 0% and 100%");
        }
        for (name, rate) in [
            ("closing costs", self.closing_cost_rate),
            ("property tax", self.property_tax_rate),
            ("maintenance", self.maintenance_rate),
        ] {
            if !(0.0..0.2).contains(&rate) {
                return bad(&format!("{name} must be between 0% and 20%"));
            }
        }
        if !(0.0..0.3).contains(&self.mortgage_rate) {
            return bad("the mortgage rate must be between 0% and 30%");
        }
        if self.down_payment_rate < 1.0 && !(1..=50).contains(&self.term_years) {
            return bad("the mortgage term must be 1 to 50 years");
        }
        if !(-0.2..=0.2).contains(&self.appreciation) {
            return bad("appreciation must be between -20% and 20%");
        }
        Ok(())
    }
}

/// The purchase in a home goal's save-up settings JSON, if it has one.
pub fn home_purchase_from_settings(settings_json: &str) -> Option<HomePurchase> {
    let value: serde_json::Value = serde_json::from_str(settings_json).ok()?;
    serde_json::from_value(value.get(HOME_PURCHASE_SETTINGS_KEY)?.clone()).ok()
}

/// A home goal's purchase, as linked into a tax-optimized plan.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct LinkedHomePurchase {
    pub goal_title: String,
    pub purchase: HomePurchase,
    /// The goal's target date (YYYY-MM-DD), used unless `purchase_age` is set.
    pub target_date: Option<String>,
}

impl LinkedHomePurchase {
    /// Age at the purchase: the purchase age, else the target date's year.
    pub fn purchase_age(&self, current_age: u32, as_of: NaiveDate) -> Option<u32> {
        if let Some(age) = self.purchase.purchase_age {
            return Some(age);
        }
        let year: i32 = self.target_date.as_deref()?.get(..4)?.parse().ok()?;
        u32::try_from(i64::from(current_age) + i64::from(year - as_of.year())).ok()
    }

    /// The purchase as expenses of the FIRE and Traditional retirement styles,
    /// in today's money growing with the home's price: the down payment and
    /// closing costs, paid from the portfolio in the purchase year even while
    /// working; the mortgage payment (fixed, so it does not grow) for its term;
    /// and property tax, insurance, maintenance and HOA from then on. Like any
    /// expense, the last two count once retired. Empty for an incomplete
    /// purchase or one already past.
    pub fn expense_items(&self, current_age: u32, as_of: NaiveDate) -> Vec<ExpenseBucket> {
        let home = &self.purchase;
        let Some(age) = self.purchase_age(current_age, as_of) else {
            return Vec::new();
        };
        if home.validate().is_err() || age < current_age {
            return Vec::new();
        }
        let years = age - current_age;
        let item =
            |key: &str, label: &str, monthly: f64, growth: f64, end: Option<u32>| ExpenseBucket {
                id: Some(format!("home-purchase-{key}")),
                label: Some(format!("{}: {label}", self.goal_title)),
                monthly_amount: monthly,
                inflation_rate: Some(growth),
                start_age: Some(age),
                end_age: end,
                essential: Some(true),
                paid_while_working: false,
                yearly: false,
                ends_at_home_purchase: false,
            };
        let mut items = vec![ExpenseBucket {
            paid_while_working: true,
            ..item(
                "upfront",
                "down payment and closing costs",
                home.price * (home.down_payment_rate + home.closing_cost_rate) / 12.0,
                home.appreciation,
                Some(age + 1),
            )
        }];
        let loan = home.price
            * (1.0 + home.appreciation).powi(years as i32)
            * (1.0 - home.down_payment_rate);
        if loan > 0.0 {
            items.push(item(
                "mortgage",
                "mortgage",
                home.monthly_payment(loan),
                0.0,
                Some(age + home.term_years),
            ));
        }
        items.push(item(
            "owning",
            "property tax, insurance, upkeep and HOA",
            (home.price * (home.property_tax_rate + home.maintenance_rate) + home.insurance) / 12.0
                + home.hoa_monthly,
            home.appreciation,
            None,
        ));
        items
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn payment_amortizes_the_loan() {
        let home = HomePurchase {
            mortgage_rate: 0.06,
            term_years: 30,
            ..HomePurchase::default()
        };
        assert!((home.monthly_payment(300_000.0) - 1_798.65).abs() < 0.01);
        let free = HomePurchase {
            mortgage_rate: 0.0,
            ..home
        };
        assert!((free.monthly_payment(360_000.0) - 1_000.0).abs() < 1e-9);
    }

    #[test]
    fn reads_the_purchase_from_save_up_settings() {
        let json =
            r#"{"targetDate":"2030-06-01","homePurchase":{"price":450000,"downPaymentRate":0.1}}"#;
        let home = home_purchase_from_settings(json).expect("purchase");
        assert_eq!(home.price, 450_000.0);
        assert_eq!(home.down_payment_rate, 0.1);
        assert_eq!(home.term_years, 30);
        assert!(home_purchase_from_settings(r#"{"targetDate":"2030-06-01"}"#).is_none());
        assert!(
            HomePurchase::default().validate().is_err(),
            "price required"
        );
    }

    fn linked(purchase_age: Option<u32>, target_date: Option<&str>) -> LinkedHomePurchase {
        LinkedHomePurchase {
            goal_title: "Lake house".into(),
            purchase: HomePurchase {
                price: 400_000.0,
                purchase_age,
                ..HomePurchase::default()
            },
            target_date: target_date.map(str::to_string),
        }
    }

    #[test]
    fn a_linked_home_becomes_expenses_from_the_purchase_age() {
        let as_of = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        // A target date in 2031 is age 50 for someone 45 today.
        let home = linked(None, Some("2031-06-01"));
        assert_eq!(home.purchase_age(45, as_of), Some(50));

        let items = home.expense_items(45, as_of);
        assert_eq!(items.len(), 3);
        let upfront = &items[0];
        assert!(upfront.paid_while_working);
        assert_eq!((upfront.start_age, upfront.end_age), (Some(50), Some(51)));
        // 20% down plus 3% closing on today's price, growing with the home.
        assert!((upfront.monthly_amount * 12.0 - 92_000.0).abs() < 1e-6);
        assert_eq!(upfront.inflation_rate, Some(0.03));

        let mortgage = &items[1];
        assert!(!mortgage.paid_while_working);
        assert_eq!((mortgage.start_age, mortgage.end_age), (Some(50), Some(80)));
        assert_eq!(mortgage.inflation_rate, Some(0.0));
        let loan = 400_000.0 * 1.03_f64.powi(5) * 0.8;
        let purchase = &home.purchase;
        assert!((mortgage.monthly_amount - purchase.monthly_payment(loan)).abs() < 1e-9);

        let owning = &items[2];
        assert_eq!((owning.start_age, owning.end_age), (Some(50), None));
        // 1% tax + 1% upkeep on 400k, plus 1,500 insurance, per month.
        assert!((owning.monthly_amount - 9_500.0 / 12.0).abs() < 1e-9);
    }

    #[test]
    fn a_past_or_unfinished_purchase_adds_nothing() {
        let as_of = NaiveDate::from_ymd_opt(2026, 1, 1).unwrap();
        assert!(linked(Some(40), None).expense_items(45, as_of).is_empty());
        assert!(linked(None, None).expense_items(45, as_of).is_empty());
        let mut unpriced = linked(Some(50), None);
        unpriced.purchase.price = 0.0;
        assert!(unpriced.expense_items(45, as_of).is_empty());
    }
}
