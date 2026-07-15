use cosmwasm_std::{Decimal as StdDecimal, Uint128};
use rust_decimal::Decimal;

use crate::{
    utils::{checked_div, checked_mul, cube_root, decimal_to_std, square_root},
    Curve, CurveError, DecimalPlaces,
};

/// spot_price is slope * (supply)^0.5
pub struct SquareRoot {
    pub slope: Decimal,
    pub normalize: DecimalPlaces,
}

impl SquareRoot {
    pub fn new(slope: Decimal, normalize: DecimalPlaces) -> Self {
        Self { slope, normalize }
    }
}

impl Curve for SquareRoot {
    fn spot_price(&self, supply: Uint128) -> Result<StdDecimal, CurveError> {
        // f(x) = self.slope * supply^0.5
        let square = self.normalize.from_supply(supply)?;
        let root = square_root(square)?;
        decimal_to_std(checked_mul(root, self.slope, "square-root spot price")?)
    }

    fn reserve(&self, supply: Uint128) -> Result<Uint128, CurveError> {
        // f(x) = self.slope * supply * supply^0.5 / 1.5
        let normalized = self.normalize.from_supply(supply)?;
        let root = square_root(normalized)?;
        let reserve = checked_div(
            checked_mul(
                checked_mul(self.slope, normalized, "square-root reserve slope")?,
                root,
                "square-root reserve root",
            )?,
            Decimal::new(15, 1),
            "square-root reserve integral",
        )?;
        self.normalize.to_reserve(reserve)
    }

    fn supply(&self, reserve: Uint128) -> Result<Uint128, CurveError> {
        // f(x) = (1.5 * reserve / self.slope) ^ (2/3)
        if self.slope.is_zero() {
            return Err(CurveError::DivisionByZero);
        }
        let base = checked_div(
            checked_mul(
                self.normalize.from_reserve(reserve)?,
                Decimal::new(15, 1),
                "square-root inverse numerator",
            )?,
            self.slope,
            "square-root inverse slope",
        )?;
        let squared = checked_mul(base, base, "square-root inverse square")?;
        let supply = cube_root(squared)?;
        self.normalize.to_supply(supply)
    }
}
