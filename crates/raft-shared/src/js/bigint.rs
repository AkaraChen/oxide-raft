//! raft_shared::js minimal unsigned big integer for exact decimal number formatting (decisions.md D3).

/// Little-endian base-2^32 unsigned integer.
#[derive(Clone, Debug)]
pub(crate) struct BigUint {
    limbs: Vec<u32>,
}

impl BigUint {
    pub(crate) fn from_u64(v: u64) -> Self {
        let mut b = BigUint {
            limbs: vec![(v & 0xFFFF_FFFF) as u32, (v >> 32) as u32],
        };
        b.normalize();
        b
    }

    /// The value of `digits` (most significant first) in radix 2^`bits`, `bits` in 1..=5.
    /// Packs the bits directly, so it is linear in the number of digits.
    pub(crate) fn from_radix_digits(digits: &[u32], bits: u32) -> Self {
        let mut limbs = Vec::with_capacity(digits.len() * 5 / 32 + 1);
        let mut acc: u64 = 0;
        let mut acc_bits = 0u32;
        for &d in digits.iter().rev() {
            acc |= u64::from(d) << acc_bits;
            acc_bits += bits;
            if acc_bits >= 32 {
                limbs.push((acc & 0xFFFF_FFFF) as u32);
                acc >>= 32;
                acc_bits -= 32;
            }
        }
        if acc_bits > 0 {
            limbs.push((acc & 0xFFFF_FFFF) as u32);
        }
        let mut b = BigUint { limbs };
        b.normalize();
        b
    }

    fn normalize(&mut self) {
        while self.limbs.last() == Some(&0) {
            self.limbs.pop();
        }
    }

    pub(crate) fn is_zero(&self) -> bool {
        self.limbs.is_empty()
    }

    pub(crate) fn mul_small(&mut self, m: u32) {
        let mut carry: u64 = 0;
        for limb in &mut self.limbs {
            let v = u64::from(*limb) * u64::from(m) + carry;
            *limb = (v & 0xFFFF_FFFF) as u32;
            carry = v >> 32;
        }
        if carry != 0 {
            self.limbs.push((carry & 0xFFFF_FFFF) as u32);
        }
        self.normalize();
    }

    pub(crate) fn add_small(&mut self, a: u32) {
        let mut carry = u64::from(a);
        for limb in &mut self.limbs {
            if carry == 0 {
                break;
            }
            let v = u64::from(*limb) + carry;
            *limb = (v & 0xFFFF_FFFF) as u32;
            carry = v >> 32;
        }
        if carry != 0 {
            self.limbs.push((carry & 0xFFFF_FFFF) as u32);
        }
    }

    pub(crate) fn shl(&mut self, bits: u32) {
        let words = (bits / 32) as usize;
        let rem = bits % 32;
        if rem != 0 {
            let mut carry = 0u32;
            for limb in &mut self.limbs {
                let v = (*limb << rem) | carry;
                carry = *limb >> (32 - rem);
                *limb = v;
            }
            if carry != 0 {
                self.limbs.push(carry);
            }
        }
        if words > 0 && !self.limbs.is_empty() {
            let mut shifted = vec![0u32; words];
            shifted.extend_from_slice(&self.limbs);
            self.limbs = shifted;
        }
    }

    pub(crate) fn shr(&mut self, bits: u32) {
        let words = (bits / 32) as usize;
        let rem = bits % 32;
        if words >= self.limbs.len() {
            self.limbs.clear();
            return;
        }
        self.limbs.drain(..words);
        if rem != 0 {
            let n = self.limbs.len();
            for i in 0..n {
                let hi = if i + 1 < n {
                    self.limbs[i + 1] << (32 - rem)
                } else {
                    0
                };
                self.limbs[i] = (self.limbs[i] >> rem) | hi;
            }
        }
        self.normalize();
    }

    pub(crate) fn bit(&self, index: u32) -> bool {
        let word = (index / 32) as usize;
        match self.limbs.get(word) {
            Some(limb) => (limb >> (index % 32)) & 1 == 1,
            None => false,
        }
    }

    fn bit_len(&self) -> u32 {
        match self.limbs.last() {
            None => 0,
            Some(top) => {
                // review: limb count of any number built here is far below 2^27, so the product fits in u32.
                let words = u32::try_from(self.limbs.len() - 1).unwrap_or(u32::MAX / 64);
                words * 32 + (32 - top.leading_zeros())
            }
        }
    }

    fn low_bits_nonzero(&self, bits: u32) -> bool {
        (0..bits).any(|i| self.bit(i))
    }

    fn low_u64(&self) -> u64 {
        let lo = u64::from(self.limbs.first().copied().unwrap_or(0));
        let hi = u64::from(self.limbs.get(1).copied().unwrap_or(0));
        lo | (hi << 32)
    }

    /// Correctly rounded (round-half-even) conversion to f64; overflow gives Infinity.
    pub(crate) fn to_f64(&self) -> f64 {
        let len = self.bit_len();
        if len <= 64 {
            // review: u64 -> f64 conversion rounds to nearest, ties to even, which is the rounding required.
            return self.low_u64() as f64;
        }
        let drop = len - 64;
        let mut top = self.clone();
        top.shr(drop);
        let mut t = top.low_u64();
        if self.low_bits_nonzero(drop) {
            // Sticky bit: the 11 spare low bits of t make this preserve correct rounding.
            t |= 1;
        }
        // review: t has 64 significant bits; the u64 -> f64 conversion rounds to nearest-even and
        // the sticky bit keeps that rounding exact; scaling by a power of two is exact until overflow.
        let scaled = t as f64;
        let exp = i32::try_from(drop).unwrap_or(i32::MAX);
        scaled * 2f64.powi(exp.min(2000))
    }

    /// Divides in place by `d`, returning the remainder.
    fn divmod_small(&mut self, d: u32) -> u32 {
        let mut rem: u64 = 0;
        for limb in self.limbs.iter_mut().rev() {
            let cur = (rem << 32) | u64::from(*limb);
            *limb = (cur / u64::from(d)) as u32;
            rem = cur % u64::from(d);
        }
        self.normalize();
        rem as u32
    }

    pub(crate) fn to_decimal(&self) -> String {
        if self.is_zero() {
            return "0".to_string();
        }
        let mut n = self.clone();
        let mut chunks = Vec::new();
        while !n.is_zero() {
            chunks.push(n.divmod_small(1_000_000_000));
        }
        let mut out = String::new();
        for (i, chunk) in chunks.iter().rev().enumerate() {
            if i == 0 {
                out.push_str(&chunk.to_string());
            } else {
                out.push_str(&format!("{chunk:09}"));
            }
        }
        out
    }
}
