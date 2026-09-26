//! raft_shared::js numeric conversions (mapping-guide §4.6): exact, checked
//! float↔integer conversions and the JS ToInt32/ToUint32 wraps. Translated code
//! uses these instead of bare `as` casts between floats and integers.

/// 2^53: integers with a magnitude at most this are exact in f64.
const TWO_POW_53: u64 = 1 << 53;

/// `x` as an i64 when it is an integer in i64's range (`-0` gives 0); None for
/// fractions, NaN, infinities, and out-of-range values.
pub fn f64_to_i64_exact(x: f64) -> Option<i64> {
    if x.trunc() != x || !(-9_223_372_036_854_775_808.0..9_223_372_036_854_775_808.0).contains(&x) {
        return None;
    }
    // review: x is an integer in [-2^63, 2^63), so the conversion is exact.
    Some(x as i64)
}

/// `x` as a u64 when it is an integer in u64's range (`-0` gives 0).
pub fn f64_to_u64_exact(x: f64) -> Option<u64> {
    if x.trunc() != x || !(0.0..18_446_744_073_709_551_616.0).contains(&x) {
        return None;
    }
    // review: x is an integer in [0, 2^64), so the conversion is exact.
    Some(x as u64)
}

/// `x` as an i32 when it is an integer in i32's range.
pub fn f64_to_i32_exact(x: f64) -> Option<i32> {
    f64_to_i64_exact(x).and_then(|v| i32::try_from(v).ok())
}

/// `x` as a u32 when it is an integer in u32's range.
pub fn f64_to_u32_exact(x: f64) -> Option<u32> {
    f64_to_u64_exact(x).and_then(|v| u32::try_from(v).ok())
}

/// `x` as a usize when it is a non-negative integer that fits.
pub fn f64_to_usize_exact(x: f64) -> Option<usize> {
    f64_to_u64_exact(x).and_then(|v| usize::try_from(v).ok())
}

/// An array or string index (`arr[i]`, `new Array(n)`): a non-negative integer
/// that fits in usize. Same as `f64_to_usize_exact`.
pub fn to_usize_index(x: f64) -> Option<usize> {
    f64_to_usize_exact(x)
}

/// `v` as f64 when every bit survives (|v| <= 2^53).
pub fn i64_to_f64_exact(v: i64) -> Option<f64> {
    if v.unsigned_abs() > TWO_POW_53 {
        return None;
    }
    // review: |v| <= 2^53, so the conversion is exact.
    Some(v as f64)
}

/// `v` as f64 when every bit survives (v <= 2^53).
pub fn u64_to_f64_exact(v: u64) -> Option<f64> {
    if v > TWO_POW_53 {
        return None;
    }
    // review: v <= 2^53, so the conversion is exact.
    Some(v as f64)
}

/// A length or count as a JS number (`.length`, `.size`). Exact up to 2^53;
/// beyond that it rounds to nearest, as a JS number would.
pub fn usize_to_f64(v: usize) -> f64 {
    // review: usize -> f64 rounds to nearest-even; exact for every length below 2^53.
    v as f64
}

/// A signed count as a JS number. Exact for |v| <= 2^53; rounds to nearest beyond.
pub fn i64_to_f64(v: i64) -> f64 {
    // review: i64 -> f64 rounds to nearest-even, which is what JS arithmetic produces.
    v as f64
}

/// A u64 as a JS number. Exact up to 2^53; rounds to nearest beyond.
pub fn u64_to_f64(v: u64) -> f64 {
    // review: u64 -> f64 rounds to nearest-even, which is what JS arithmetic produces.
    v as f64
}

/// ToUint32 (`x >>> 0`).
pub fn to_uint32(x: f64) -> u32 {
    if !x.is_finite() {
        return 0;
    }
    let m = x.trunc().rem_euclid(4_294_967_296.0);
    // m is an integer in [0, 2^32), so the conversion is exact.
    f64_to_u32_exact(m).unwrap_or(0)
}

/// ToInt32 (`x | 0`).
pub fn to_int32(x: f64) -> i32 {
    i32::from_ne_bytes(to_uint32(x).to_ne_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_conversions() {
        assert_eq!(f64_to_i64_exact(-0.0), Some(0));
        assert_eq!(f64_to_i64_exact(1.5), None);
        assert_eq!(f64_to_i64_exact(f64::NAN), None);
        assert_eq!(f64_to_i64_exact(9_223_372_036_854_775_808.0), None);
        assert_eq!(
            f64_to_i64_exact(-9_223_372_036_854_775_808.0),
            Some(i64::MIN)
        );
        assert_eq!(f64_to_u64_exact(-1.0), None);
        assert_eq!(f64_to_u32_exact(4_294_967_295.0), Some(u32::MAX));
        assert_eq!(f64_to_u32_exact(4_294_967_296.0), None);
        assert_eq!(f64_to_i32_exact(-2_147_483_648.0), Some(i32::MIN));
        assert_eq!(to_usize_index(f64::INFINITY), None);
        assert_eq!(i64_to_f64_exact(1 << 53), Some(9_007_199_254_740_992.0));
        assert_eq!(i64_to_f64_exact((1 << 53) + 1), None);
        assert_eq!(u64_to_f64_exact(u64::MAX), None);
        assert_eq!(usize_to_f64(12), 12.0);
    }

    #[test]
    fn js_wraps() {
        assert_eq!(to_uint32(-1.0), u32::MAX);
        assert_eq!(to_int32(4_294_967_295.0), -1);
        assert_eq!(to_int32(2_147_483_648.5), i32::MIN);
        assert_eq!(to_int32(f64::NAN), 0);
        assert_eq!(to_uint32(-0.5), 0);
    }
}
