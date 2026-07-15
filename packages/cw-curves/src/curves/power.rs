use cosmwasm_std::{Decimal as StdDecimal, Uint128};
use rust_decimal::Decimal;

use crate::{
    utils::{checked_div, checked_mul, decimal_to_std, pow_rational},
    Curve, CurveError, DecimalPlaces,
};

/// `f(s) = slope * s^(num/den)`. Generalizes `Constant` (n=0/1), `Linear`
/// (n=1/1), and `SquareRoot` (n=1/2) under a single curve with a rational
/// exponent. Phase U addition.
///
/// Integral (the `reserve` function): `F(s) = slope * s^((num+den)/den) /
/// ((num + den) / den) = slope * den / (num + den) * s^((num+den)/den)`.
///
/// Inverse (the `supply` function): `F^-1(r) = ((num + den) * r / (slope *
/// den))^(den / (num + den))`.
pub struct Power {
    pub slope: Decimal,
    pub exponent_num: u32,
    pub exponent_den: u32,
    pub normalize: DecimalPlaces,
}

impl Power {
    const MAX_EXPONENT_WORK: u32 = 32;

    pub fn new(
        slope: Decimal,
        exponent_num: u32,
        exponent_den: u32,
        normalize: DecimalPlaces,
    ) -> Self {
        Self {
            slope,
            exponent_num,
            exponent_den,
            normalize,
        }
    }

    /// (num + den) / den, used in integral and inverse.
    fn integral_num(&self) -> Result<u32, CurveError> {
        let sum = self
            .exponent_num
            .checked_add(self.exponent_den)
            .ok_or_else(|| CurveError::InvalidConfiguration {
                reason: "power exponent sum overflows".into(),
            })?;
        if self.exponent_den == 0
            || self.exponent_num > Self::MAX_EXPONENT_WORK
            || self.exponent_den > Self::MAX_EXPONENT_WORK
            || sum > Self::MAX_EXPONENT_WORK
        {
            return Err(CurveError::InvalidConfiguration {
                reason: "power denominator must be positive and exponent work must be <= 32".into(),
            });
        }
        Ok(sum)
    }

    fn validate_exponents(&self) -> Result<(), CurveError> {
        self.integral_num().map(|_| ())
    }
}

impl Curve for Power {
    fn spot_price(&self, supply: Uint128) -> Result<StdDecimal, CurveError> {
        self.validate_exponents()?;
        // f(x) = slope * supply^(num/den)
        let s = self.normalize.from_supply(supply)?;
        let powered = pow_rational(s, self.exponent_num, self.exponent_den)?;
        decimal_to_std(checked_mul(self.slope, powered, "power spot price")?)
    }

    fn reserve(&self, supply: Uint128) -> Result<Uint128, CurveError> {
        // F(s) = slope * den / (num + den) * s^((num + den) / den)
        let s = self.normalize.from_supply(supply)?;
        let integral_num = self.integral_num()?;
        let powered = pow_rational(s, integral_num, self.exponent_den)?;
        let coefficient = checked_div(
            checked_mul(
                self.slope,
                Decimal::from(self.exponent_den),
                "power reserve coefficient numerator",
            )?,
            Decimal::from(integral_num),
            "power reserve coefficient",
        )?;
        self.normalize
            .to_reserve(checked_mul(coefficient, powered, "power reserve result")?)
    }

    fn supply(&self, reserve: Uint128) -> Result<Uint128, CurveError> {
        // F^-1(r) = ((num + den) * r / (slope * den))^(den / (num + den))
        if self.slope.is_zero() {
            return Err(CurveError::DivisionByZero);
        }
        let r = self.normalize.from_reserve(reserve)?;
        let integral_num = self.integral_num()?;
        let numerator = checked_mul(Decimal::from(integral_num), r, "power inverse numerator")?;
        let denominator = checked_mul(
            self.slope,
            Decimal::from(self.exponent_den),
            "power inverse denominator",
        )?;
        if denominator.is_zero() {
            return Err(CurveError::DivisionByZero);
        }
        let base = checked_div(numerator, denominator, "power inverse base")?;
        let supply = pow_rational(base, self.exponent_den, integral_num)?;
        self.normalize.to_supply(supply)
    }
}
