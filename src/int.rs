//! This module contains an 256-bit signed integer implementation.

use crate::errors::{ParseI256Error, TryFromBigIntError};
use std::cmp;
use std::convert::{TryFrom, TryInto};
use std::fmt;
use std::i128;
use std::str::{self, FromStr};
use web3::types::U256;

/// Compute the two's complement of a U256.
fn twos_complement(u: U256) -> U256 {
    let (twos_complement, _) = (!u).overflowing_add(U256::one());
    twos_complement
}

/// Little-endian 256-bit signed integer.
#[derive(Clone, Copy, Default, Eq, Hash, Ord, PartialEq)]
pub struct I256(U256);

/// Enum to represent the sign of a 256-bit signed integer.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Sign {
    /// Greater than or equal to zero.
    Positive,
    /// Less than zero.
    Negative,
}

impl I256 {
    /// Creates an I256 from a sign and an absolute value. Returns the value and
    /// a bool that is true if the conversion caused an overflow.
    fn overflowing_from_sign_and_abs(sign: Sign, abs: U256) -> (Self, bool) {
        let value = I256(match sign {
            Sign::Positive => abs,
            Sign::Negative => twos_complement(abs),
        });
        (value, value.sign() != sign)
    }

    /// Creates an I256 from an absolute value and a negative flag. Returns
    /// `None` if it would overflow an `I256`.
    fn checked_from_sign_and_abs(sign: Sign, abs: U256) -> Option<Self> {
        let (result, overflow) = I256::overflowing_from_sign_and_abs(sign, abs);
        if overflow {
            None
        } else {
            Some(result)
        }
    }

    /// Splits a I256 into its absolute value and negative flag.
    fn into_sign_and_abs(self) -> (Sign, U256) {
        let sign = self.sign();
        let abs = match sign {
            Sign::Positive => self.0,
            Sign::Negative => twos_complement(self.0),
        };
        (sign, abs)
    }

    /// Returns the sign of self.
    fn sign(self) -> Sign {
        let most_significant_word = (self.0).0[3];
        match most_significant_word & (1 << 63) {
            0 => Sign::Positive,
            _ => Sign::Negative,
        }
    }

    /// Returns the signed integer as a unsigned integer. If the value of `self`
    /// negative, then the two's complement of its absolute value will be
    /// returned.
    pub fn into_raw(self) -> U256 {
        self.0
    }

    /// Conversion to i32
    pub fn low_i32(&self) -> i32 {
        self.0.low_u32() as _
    }

    /// Conversion to i64
    pub fn low_i64(&self) -> i64 {
        self.0.low_u64() as _
    }

    /// Conversion to i128
    pub fn low_i128(&self) -> i128 {
        self.0.low_u128() as _
    }

    /// Conversion to ui2 with overflow checking
    ///
    /// # Panics
    ///
    /// Panics if the number is outside the range [`i32::MIN`, `i32::MAX`].
    pub fn as_i32(&self) -> i32 {
        (*self).try_into().unwrap()
    }

    /// Conversion to i64 with overflow checking
    ///
    /// # Panics
    ///
    /// Panics if the number is outside the range [`i64::MIN`, `i64::MAX`].
    pub fn as_i64(&self) -> i64 {
        (*self).try_into().unwrap()
    }

    /// Conversion to i128 with overflow checking
    ///
    /// # Panics
    ///
    /// Panics if the number is outside the range [`i128::MIN`, `i128::MAX`].
    pub fn as_i128(&self) -> i128 {
        (*self).try_into().unwrap()
    }

    /// Conversion to usize with overflow checking
    ///
    /// # Panics
    ///
    /// Panics if the number is outside the range [`isize::MIN`, `isize::MAX`].
    pub fn as_isize(&self) -> usize {
        (*self).try_into().unwrap()
    }

    /// Convert from a decimal string.
    pub fn from_dec_str(value: &str) -> Result<Self, ParseI256Error> {
        let (sign, value) = match value.as_bytes().get(0) {
            Some(b'+') => (Sign::Positive, &value[1..]),
            Some(b'-') => (Sign::Negative, &value[1..]),
            _ => (Sign::Positive, value),
        };

        let abs = U256::from_dec_str(value)?;
        let result =
            I256::checked_from_sign_and_abs(sign, abs).ok_or(ParseI256Error::IntegerOverflow)?;

        Ok(result)
    }

    /// Convert from a hexadecimal string.
    pub fn from_hex_str(value: &str) -> Result<Self, ParseI256Error> {
        let (sign, value) = match value.as_bytes().get(0) {
            Some(b'+') => (Sign::Positive, &value[1..]),
            Some(b'-') => (Sign::Negative, &value[1..]),
            _ => (Sign::Positive, value),
        };

        // NOTE: Do the hex conversion here as `U256` implementation can panic.
        if value.len() > 64 {
            return Err(ParseI256Error::IntegerOverflow);
        }
        let mut abs = U256::zero();
        for (i, word) in value.as_bytes().rchunks(16).enumerate() {
            let word = str::from_utf8(word).map_err(|_| ParseI256Error::InvalidDigit)?;
            abs.0[i] = u64::from_str_radix(word, 16).map_err(|_| ParseI256Error::InvalidDigit)?;
        }

        let result =
            I256::checked_from_sign_and_abs(sign, abs).ok_or(ParseI256Error::IntegerOverflow)?;

        Ok(result)
    }
}
macro_rules! impl_std_int_from_and_into {
    ($( $t:ty ),*) => {
        $(
            impl From<$t> for I256 {
                fn from(value: $t) -> Self {
                    #[allow(unused_comparisons)]
                    I256(if value < 0 {
                        twos_complement(U256::from(0 - value))
                    } else {
                        U256::from(value)
                    })
                }
            }

            impl TryInto<$t> for I256 {
                type Error = TryFromBigIntError;

                fn try_into(self) -> Result<$t, Self::Error> {
                    if self < I256::from(<$t>::min_value()) ||
                        self > I256::from(<$t>::max_value()) {
                        return Err(TryFromBigIntError);
                    }

                    Ok(self.0.low_u128() as _)
                }
            }
        )*
    };
}

impl_std_int_from_and_into!(u8, u32, u64, u128, usize, i8, i32, i64, i128, isize);

impl TryFrom<U256> for I256 {
    type Error = TryFromBigIntError;

    fn try_from(from: U256) -> Result<Self, Self::Error> {
        let value = I256(from);
        match value.sign() {
            Sign::Positive => Ok(value),
            Sign::Negative => Err(TryFromBigIntError),
        }
    }
}

impl TryInto<U256> for I256 {
    type Error = TryFromBigIntError;

    fn try_into(self) -> Result<U256, Self::Error> {
        match self.sign() {
            Sign::Positive => Ok(self.0),
            Sign::Negative => Err(TryFromBigIntError),
        }
    }
}

impl FromStr for I256 {
    type Err = ParseI256Error;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        I256::from_hex_str(value)
    }
}

impl fmt::Debug for I256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        fmt::Display::fmt(self, f)
    }
}

impl fmt::Display for Sign {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match (self, f.sign_plus()) {
            (Sign::Positive, false) => Ok(()),
            (Sign::Positive, true) => write!(f, "+"),
            (Sign::Negative, _) => write!(f, "-"),
        }
    }
}

impl fmt::Display for I256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (sign, abs) = self.into_sign_and_abs();
        sign.fmt(f)?;
        write!(f, "{}", abs)
    }
}

impl fmt::LowerHex for I256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (sign, abs) = self.into_sign_and_abs();
        fmt::Display::fmt(&sign, f)?;
        write!(f, "{:x}", abs)
    }
}

impl fmt::UpperHex for I256 {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let (sign, abs) = self.into_sign_and_abs();
        fmt::Display::fmt(&sign, f)?;

        // NOTE: Work around `U256: !UpperHex`.
        let mut buffer = format!("{:x}", abs);
        buffer.make_ascii_uppercase();
        write!(f, "{}", buffer)
    }
}

impl cmp::PartialOrd for I256 {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        // TODO: Once subtraction is implemented:
        // self.saturating_sub(*other).signum64().partial_cmp(&0)

        use cmp::Ordering::*;
        use Sign::*;

        let ord = match (self.into_sign_and_abs(), other.into_sign_and_abs()) {
            ((Positive, _), (Negative, _)) => Greater,
            ((Negative, _), (Positive, _)) => Less,
            ((Positive, this), (Positive, other)) => this.cmp(&other),
            ((Negative, this), (Negative, other)) => other.cmp(&this),
        };

        Some(ord)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazy_static::lazy_static;

    lazy_static! {
        static ref MIN_ABS: U256 = U256::from(1) << 255;
    }

    #[test]
    fn parse_dec_str() {
        let unsigned = U256::from_dec_str("314159265358979323846264338327950288419716").unwrap();

        let value = I256::from_dec_str(&format!("-{}", unsigned)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Negative, unsigned));

        let value = I256::from_dec_str(&format!("{}", unsigned)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Positive, unsigned));

        let value = I256::from_dec_str(&format!("+{}", unsigned)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Positive, unsigned));

        let err = I256::from_dec_str("invalid string").unwrap_err();
        assert!(matches!(err, ParseI256Error::InvalidDigit));

        let err = I256::from_dec_str(&format!("1{}", U256::MAX)).unwrap_err();
        assert!(matches!(err, ParseI256Error::IntegerOverflow));

        let err = I256::from_dec_str(&format!("-{}", U256::MAX)).unwrap_err();
        assert!(matches!(err, ParseI256Error::IntegerOverflow));

        let value = I256::from_dec_str(&format!("-{}", *MIN_ABS)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Negative, *MIN_ABS));

        let err = I256::from_dec_str(&format!("{}", *MIN_ABS)).unwrap_err();
        assert!(matches!(err, ParseI256Error::IntegerOverflow));
    }

    #[test]
    fn parse_hex_str() {
        let unsigned = U256::from_dec_str("314159265358979323846264338327950288419716").unwrap();

        let value = I256::from_hex_str(&format!("-{:x}", unsigned)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Negative, unsigned));

        let value = I256::from_hex_str(&format!("{:x}", unsigned)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Positive, unsigned));

        let value = I256::from_hex_str(&format!("+{:x}", unsigned)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Positive, unsigned));

        let err = I256::from_hex_str("invalid string").unwrap_err();
        assert!(matches!(err, ParseI256Error::InvalidDigit));

        let err = I256::from_hex_str(&format!("1{:x}", U256::MAX)).unwrap_err();
        assert!(matches!(err, ParseI256Error::IntegerOverflow));

        let err = I256::from_hex_str(&format!("-{:x}", U256::MAX)).unwrap_err();
        assert!(matches!(err, ParseI256Error::IntegerOverflow));

        let value = I256::from_hex_str(&format!("-{:x}", *MIN_ABS)).unwrap();
        assert_eq!(value.into_sign_and_abs(), (Sign::Negative, *MIN_ABS));

        let err = I256::from_hex_str(&format!("{:x}", *MIN_ABS)).unwrap_err();
        assert!(matches!(err, ParseI256Error::IntegerOverflow));
    }

    #[test]
    fn formatting() {
        let unsigned = U256::from_dec_str("314159265358979323846264338327950288419716").unwrap();

        let positive = I256::checked_from_sign_and_abs(Sign::Positive, unsigned).unwrap();
        let negative = I256::checked_from_sign_and_abs(Sign::Negative, unsigned).unwrap();

        assert_eq!(format!("{}", positive), format!("{}", unsigned));
        assert_eq!(format!("{}", negative), format!("-{}", unsigned));
        assert_eq!(format!("{:+}", positive), format!("+{}", unsigned));
        assert_eq!(format!("{:+}", negative), format!("-{}", unsigned));

        assert_eq!(format!("{:x}", positive), format!("{:x}", unsigned));
        assert_eq!(format!("{:x}", negative), format!("-{:x}", unsigned));
        assert_eq!(format!("{:+x}", positive), format!("+{:x}", unsigned));
        assert_eq!(format!("{:+x}", negative), format!("-{:x}", unsigned));

        assert_eq!(
            format!("{:X}", positive),
            format!("{:x}", unsigned).to_uppercase()
        );
        assert_eq!(
            format!("{:X}", negative),
            format!("-{:x}", unsigned).to_uppercase()
        );
        assert_eq!(
            format!("{:+X}", positive),
            format!("+{:x}", unsigned).to_uppercase()
        );
        assert_eq!(
            format!("{:+X}", negative),
            format!("-{:x}", unsigned).to_uppercase()
        );
    }
}
