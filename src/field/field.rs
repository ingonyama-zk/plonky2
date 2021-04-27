use std::fmt::{Debug, Display};
use std::hash::Hash;
use std::iter::{Product, Sum};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

use num::Integer;
use rand::rngs::OsRng;
use rand::Rng;

use crate::util::bits_u64;

/// A finite field with prime order less than 2^64.
pub trait Field:
    'static
    + Copy
    + Eq
    + Hash
    + Neg<Output = Self>
    + Add<Self, Output = Self>
    + AddAssign<Self>
    + Sum
    + Sub<Self, Output = Self>
    + SubAssign<Self>
    + Mul<Self, Output = Self>
    + MulAssign<Self>
    + Product
    + Div<Self, Output = Self>
    + DivAssign<Self>
    + Debug
    + Display
    + Send
    + Sync
{
    const ZERO: Self;
    const ONE: Self;
    const TWO: Self;
    const NEG_ONE: Self;

    const ORDER: u64;
    const TWO_ADICITY: usize;

    /// Generator of the entire multiplicative group, i.e. all non-zero elements.
    const MULTIPLICATIVE_GROUP_GENERATOR: Self;
    /// Generator of a multiplicative subgroup of order `2^TWO_ADICITY`.
    const POWER_OF_TWO_GENERATOR: Self;

    fn is_zero(&self) -> bool {
        *self == Self::ZERO
    }

    fn is_nonzero(&self) -> bool {
        *self != Self::ZERO
    }

    fn is_one(&self) -> bool {
        *self == Self::ONE
    }

    fn square(&self) -> Self {
        *self * *self
    }

    fn cube(&self) -> Self {
        *self * *self * *self
    }

    /// Compute the multiplicative inverse of this field element.
    fn try_inverse(&self) -> Option<Self>;

    fn inverse(&self) -> Self {
        self.try_inverse().expect("Tried to invert zero")
    }

    fn batch_multiplicative_inverse(x: &[Self]) -> Vec<Self> {
        // This is Montgomery's trick. At a high level, we invert the product of the given field
        // elements, then derive the individual inverses from that via multiplication.

        let n = x.len();
        if n == 0 {
            return Vec::new();
        }

        let mut a = Vec::with_capacity(n);
        a.push(x[0]);
        for i in 1..n {
            a.push(a[i - 1] * x[i]);
        }

        let mut a_inv = vec![Self::ZERO; n];
        a_inv[n - 1] = a[n - 1].try_inverse().expect("No inverse");
        for i in (0..n - 1).rev() {
            a_inv[i] = x[i + 1] * a_inv[i + 1];
        }

        let mut x_inv = Vec::with_capacity(n);
        x_inv.push(a_inv[0]);
        for i in 1..n {
            x_inv.push(a[i - 1] * a_inv[i]);
        }
        x_inv
    }

    fn primitive_root_of_unity(n_log: usize) -> Self {
        assert!(n_log <= Self::TWO_ADICITY);
        let mut base = Self::POWER_OF_TWO_GENERATOR;
        for _ in n_log..Self::TWO_ADICITY {
            base = base.square();
        }
        base
    }

    /// Computes a multiplicative subgroup whose order is known in advance.
    fn cyclic_subgroup_known_order(generator: Self, order: usize) -> Vec<Self> {
        let mut subgroup = Vec::with_capacity(order);
        let mut current = Self::ONE;
        for _i in 0..order {
            subgroup.push(current);
            current *= generator;
        }
        subgroup
    }

    fn cyclic_subgroup_unknown_order(generator: Self) -> Vec<Self> {
        let mut subgroup = Vec::new();
        for power in generator.powers() {
            if power.is_one() && !subgroup.is_empty() {
                break;
            }
            subgroup.push(power);
        }
        subgroup
    }

    fn generator_order(generator: Self) -> usize {
        generator.powers().skip(1).position(|y| y.is_one()).unwrap() + 1
    }

    /// Computes a coset of a multiplicative subgroup whose order is known in advance.
    fn cyclic_subgroup_coset_known_order(generator: Self, shift: Self, order: usize) -> Vec<Self> {
        let subgroup = Self::cyclic_subgroup_known_order(generator, order);
        subgroup.into_iter().map(|x| x * shift).collect()
    }

    fn to_canonical_u64(&self) -> u64;

    fn from_canonical_u64(n: u64) -> Self;

    fn from_canonical_u32(n: u32) -> Self {
        Self::from_canonical_u64(n as u64)
    }

    fn from_canonical_usize(n: usize) -> Self {
        Self::from_canonical_u64(n as u64)
    }

    fn bits(&self) -> usize {
        bits_u64(self.to_canonical_u64())
    }

    fn exp(&self, power: Self) -> Self {
        let mut current = *self;
        let mut product = Self::ONE;

        for j in 0..power.bits() {
            if (power.to_canonical_u64() >> j & 1) != 0 {
                product *= current;
            }
            current = current.square();
        }
        product
    }

    fn exp_u32(&self, power: u32) -> Self {
        self.exp(Self::from_canonical_u32(power))
    }

    fn exp_usize(&self, power: usize) -> Self {
        self.exp(Self::from_canonical_usize(power))
    }

    /// Returns whether `x^power` is a permutation of this field.
    fn is_monomial_permutation(power: Self) -> bool {
        if power.is_zero() {
            return false;
        }
        if power.is_one() {
            return true;
        }
        (Self::ORDER - 1).gcd(&power.to_canonical_u64()) == 1
    }

    fn kth_root(&self, k: Self) -> Self {
        let p = Self::ORDER;
        let p_minus_1 = p - 1;
        debug_assert!(
            Self::is_monomial_permutation(k),
            "Not a permutation of this field"
        );
        let k = k.to_canonical_u64();

        // By Fermat's little theorem, x^p = x and x^(p - 1) = 1, so x^(p + n(p - 1)) = x for any n.
        // Our assumption that the k'th root operation is a permutation implies gcd(p - 1, k) = 1,
        // so there exists some n such that p + n(p - 1) is a multiple of k. Once we find such an n,
        // we can rewrite the above as
        //    x^((p + n(p - 1))/k)^k = x,
        // implying that x^((p + n(p - 1))/k) is a k'th root of x.
        for n in 0..k {
            let numerator = p as u128 + n as u128 * p_minus_1 as u128;
            if numerator % k as u128 == 0 {
                let power = (numerator / k as u128) as u64 % p_minus_1;
                return self.exp(Self::from_canonical_u64(power));
            }
        }
        panic!(
            "x^{} and x^(1/{}) are not permutations of this field, or we have a bug!",
            k, k
        );
    }

    fn kth_root_u32(&self, k: u32) -> Self {
        self.kth_root(Self::from_canonical_u32(k))
    }

    fn cube_root(&self) -> Self {
        self.kth_root_u32(3)
    }

    fn powers(&self) -> Powers<Self> {
        Powers {
            base: *self,
            current: Self::ONE,
        }
    }

    fn rand_from_rng<R: Rng>(rng: &mut R) -> Self {
        Self::from_canonical_u64(rng.gen_range(0, Self::ORDER))
    }

    fn rand() -> Self {
        Self::rand_from_rng(&mut OsRng)
    }
}

/// An iterator over the powers of a certain base element `b`: `b^0, b^1, b^2, ...`.
pub struct Powers<F: Field> {
    base: F,
    current: F,
}

impl<F: Field> Iterator for Powers<F> {
    type Item = F;

    fn next(&mut self) -> Option<F> {
        let result = self.current;
        self.current *= self.base;
        Some(result)
    }
}
