//! Exact rational numbers used for frame rates and timing math.

use std::fmt;

use crate::error::TypesError;

/// Reduced rational `num/den`. Denominator is always non-zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rational {
    num: u32,
    den: u32,
}

impl Rational {
    /// Constructs and reduces a rational. Rejects a zero denominator.
    pub fn new(num: u32, den: u32) -> Result<Self, TypesError> {
        if den == 0 {
            return Err(TypesError::ZeroDenominator);
        }
        Ok(Self::reduce(num, den))
    }

    /// Integer rational `value/1`.
    pub const fn from_integer(value: u32) -> Self {
        Self { num: value, den: 1 }
    }

    pub const fn num(self) -> u32 {
        self.num
    }

    pub const fn den(self) -> u32 {
        self.den
    }

    pub const fn is_integer(self) -> bool {
        self.den == 1
    }

    fn reduce(num: u32, den: u32) -> Self {
        let g = gcd(num, den);
        Self {
            num: num / g,
            den: den / g,
        }
    }

    /// Period of one cycle in nanoseconds (e.g. frame interval from FPS).
    pub fn period_nanos(self) -> Result<u64, TypesError> {
        if self.num == 0 {
            return Err(TypesError::InvalidRational);
        }
        let nanos = 1_000_000_000u128.saturating_mul(u128::from(self.den)) / u128::from(self.num);
        Ok(clamp_u64(nanos))
    }

    /// `count` cycles at this rate, in nanoseconds.
    pub fn duration_nanos_for(self, count: u64) -> Result<u64, TypesError> {
        if self.num == 0 {
            return Err(TypesError::InvalidRational);
        }
        let nanos = u128::from(count)
            .saturating_mul(1_000_000_000)
            .saturating_mul(u128::from(self.den))
            / u128::from(self.num);
        Ok(clamp_u64(nanos))
    }

    /// How many whole cycles fit in `duration_ns`.
    pub fn cycles_in(self, duration_ns: u64) -> Result<u64, TypesError> {
        if self.den == 0 {
            return Err(TypesError::ZeroDenominator);
        }
        let count = u128::from(duration_ns).saturating_mul(u128::from(self.num))
            / (1_000_000_000u128.saturating_mul(u128::from(self.den)));
        Ok(clamp_u64(count))
    }
}

impl fmt::Display for Rational {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.den == 1 {
            write!(f, "{}", self.num)
        } else {
            write!(f, "{}/{}", self.num, self.den)
        }
    }
}

const fn gcd(mut a: u32, mut b: u32) -> u32 {
    while b != 0 {
        let t = b;
        b = a % b;
        a = t;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

fn clamp_u64(value: u128) -> u64 {
    if value > u128::from(u64::MAX) {
        u64::MAX
    } else {
        value as u64
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn rejects_zero_denominator() {
        assert_eq!(Rational::new(24, 0), Err(TypesError::ZeroDenominator));
    }

    #[test]
    fn reduces_fraction() {
        let r = Rational::new(120, 2).unwrap();
        assert_eq!(r.num(), 60);
        assert_eq!(r.den(), 1);
    }

    #[test]
    fn one_hundred_forty_four_fps_period() {
        let fps = Rational::new(144, 1).unwrap();
        assert_eq!(fps.period_nanos().unwrap(), 1_000_000_000 / 144);
        assert_eq!(fps.cycles_in(10_000_000_000).unwrap(), 1_440);
        assert_eq!(fps.duration_nanos_for(1_440).unwrap(), 10_000_000_000);
    }

    #[test]
    fn gcd_zero_numerator() {
        let r = Rational::new(0, 5).unwrap();
        assert_eq!(r.num(), 0);
        assert_eq!(r.den(), 1);
    }
}
