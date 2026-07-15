use cosmwasm_std::{Decimal as StdDecimal, Uint128};
use rust_decimal::Decimal;

use crate::{
    utils::{
        checked_add, checked_div, checked_mul, checked_neg, checked_sub, decimal_to_std, taylor_exp,
    },
    Curve, CurveError, DecimalPlaces,
};

/// Sigmoid (logistic) bonding curve. Spot price follows the standard logistic
/// shape, asymptotic to `amplitude` as supply grows large, with inflection
/// at `midpoint` and slope controlled by `steepness`:
///
/// `f(s) = amplitude / (1 + e^(-steepness * (s - midpoint)))`
///
/// Phase V addition. Used in production by Token Engineering Commons for
/// smoothed price discovery. See `audits/2026-05-09-cw-abc-security-review.md`
/// for the audit context.
///
/// **Numerical strategy.** The closed-form integral exists but requires
/// `softplus(x) = ln(1 + e^x)`, which would force a `taylor_ln` helper. To
/// keep the impl smaller and more easily auditable, we use Simpson's rule
/// numerical integration (16 panels) over the curve and Newton-Raphson on
/// the integral for the inverse.
///
/// **Precision.** Simpson's rule has `O(h^4)` error on smooth functions;
/// for 16 panels over a typical input range, expect ~1e-3 relative error.
/// Newton inverse is bounded at 16 iterations.
///
/// **Constraints validated at construction:**
/// - `amplitude > 0`
/// - `steepness > 0`
/// - `|steepness * (max_supply - midpoint)| ≤ 30` (see `taylor_exp`).
pub struct Sigmoid {
    pub amplitude: Decimal,
    pub steepness: Decimal,
    pub midpoint: Decimal,
    pub normalize: DecimalPlaces,
}

impl Sigmoid {
    pub fn new(
        amplitude: Decimal,
        steepness: Decimal,
        midpoint: Decimal,
        normalize: DecimalPlaces,
    ) -> Self {
        Self {
            amplitude,
            steepness,
            midpoint,
            normalize,
        }
    }

    /// Spot price at normalized supply `s` (Decimal). Inner helper used by
    /// both `spot_price` and the integration / Newton paths.
    fn price_at(&self, s: Decimal) -> Result<Decimal, CurveError> {
        let offset = checked_sub(s, self.midpoint, "sigmoid supply offset")?;
        let exponent = checked_neg(
            checked_mul(self.steepness, offset, "sigmoid exponent")?,
            "sigmoid negative exponent",
        )?;
        let denom = checked_add(
            Decimal::ONE,
            taylor_exp(exponent)?,
            "sigmoid price denominator",
        )?;
        if denom.is_zero() {
            return Err(CurveError::DivisionByZero);
        }
        checked_div(self.amplitude, denom, "sigmoid price")
    }

    /// Simpson's rule on `[0, supply]` with `panels` (must be even) panels.
    fn integrate(&self, supply: Decimal, panels: u32) -> Result<Decimal, CurveError> {
        if supply.is_zero() {
            return Ok(Decimal::ZERO);
        }
        let panels = if panels % 2 == 0 { panels } else { panels + 1 };
        let h = checked_div(supply, Decimal::from(panels), "sigmoid panel width")?;
        // Simpson: (h/3) * (f0 + 4*(f1+f3+f5+...) + 2*(f2+f4+...) + fn)
        let f0 = self.price_at(Decimal::ZERO)?;
        let fn_end = self.price_at(supply)?;
        let mut odd_sum = Decimal::ZERO;
        let mut even_sum = Decimal::ZERO;
        for i in 1..panels {
            let s = checked_mul(h, Decimal::from(i), "sigmoid panel position")?;
            let f = self.price_at(s)?;
            if i % 2 == 1 {
                odd_sum = checked_add(odd_sum, f, "sigmoid odd panel sum")?;
            } else {
                even_sum = checked_add(even_sum, f, "sigmoid even panel sum")?;
            }
        }
        let four = Decimal::from(4u32);
        let two = Decimal::from(2u32);
        let three = Decimal::from(3u32);
        let weighted_odd = checked_mul(four, odd_sum, "sigmoid weighted odd panels")?;
        let weighted_even = checked_mul(two, even_sum, "sigmoid weighted even panels")?;
        let sum = checked_add(
            checked_add(
                checked_add(f0, weighted_odd, "sigmoid Simpson f0 and odd")?,
                weighted_even,
                "sigmoid Simpson even",
            )?,
            fn_end,
            "sigmoid Simpson endpoint",
        )?;
        checked_mul(
            checked_div(h, three, "sigmoid Simpson panel factor")?,
            sum,
            "sigmoid integral",
        )
    }

    pub(crate) fn supply_with_iterations(
        &self,
        reserve: Uint128,
        max_iterations: u32,
    ) -> Result<Uint128, CurveError> {
        if self.amplitude.is_zero() || self.steepness.is_zero() {
            return Err(CurveError::DivisionByZero);
        }
        let target = self.normalize.from_reserve(reserve)?;
        if target.is_zero() {
            return Ok(Uint128::zero());
        }
        // Newton-Raphson on g(s) = integrate(s) - target = 0; g'(s) = price_at(s).
        let mut s = self.midpoint;
        if s <= Decimal::ZERO {
            s = Decimal::ONE;
        }
        for _ in 0..max_iterations {
            let r = self.integrate(s, 16)?;
            let derivative = self.price_at(s)?;
            if derivative.is_zero() {
                return Err(CurveError::DivisionByZero);
            }
            let correction = checked_div(
                checked_sub(r, target, "sigmoid inverse residual")?,
                derivative,
                "sigmoid inverse correction",
            )?;
            let next = checked_sub(s, correction, "sigmoid inverse next supply")?;
            let next = if next < Decimal::ZERO {
                checked_div(s, Decimal::from(2u32), "sigmoid inverse backoff")?
            } else {
                next
            };
            let diff = if next > s {
                checked_sub(next, s, "sigmoid inverse positive difference")?
            } else {
                checked_sub(s, next, "sigmoid inverse negative difference")?
            };
            s = next;
            if diff < Decimal::new(1, 9) {
                return self.normalize.to_supply(s);
            }
        }
        Err(CurveError::NonConvergence {
            operation: "sigmoid inverse".into(),
        })
    }
}

impl Curve for Sigmoid {
    fn spot_price(&self, supply: Uint128) -> Result<StdDecimal, CurveError> {
        let s = self.normalize.from_supply(supply)?;
        let p = self.price_at(s)?;
        decimal_to_std(p)
    }

    fn reserve(&self, supply: Uint128) -> Result<Uint128, CurveError> {
        let s = self.normalize.from_supply(supply)?;
        let r = self.integrate(s, 32)?;
        self.normalize.to_reserve(r)
    }

    fn supply(&self, reserve: Uint128) -> Result<Uint128, CurveError> {
        const MAX_ITERS: u32 = 32;
        self.supply_with_iterations(reserve, MAX_ITERS)
    }
}
