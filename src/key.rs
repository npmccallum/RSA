use num_bigint::{BigUint, RandBigInt};
use num_traits::{FromPrimitive, One, Zero};
use rand::Rng;

use algorithms::generate_multi_prime_key;
use errors::Result;
use math::ModInverse;
use padding::PaddingScheme;
use pkcs1v15;

/// Represents the public part of an RSA key.
#[derive(Debug, Clone)]
pub struct RSAPublicKey {
    n: BigUint,
    e: u32,
}

/// Represents a whole RSA key, public and private parts.
#[derive(Debug, Clone)]
pub struct RSAPrivateKey {
    /// Modulus
    n: BigUint,
    /// Public exponent
    e: u32,
    /// Private exponent
    d: BigUint,
    /// Prime factors of N, contains >= 2 elements.
    primes: Vec<BigUint>,
}

impl From<RSAPrivateKey> for RSAPublicKey {
    fn from(private_key: RSAPrivateKey) -> Self {
        RSAPublicKey {
            n: private_key.n.clone(),
            e: private_key.e,
        }
    }
}

/// Generic trait for operations on a public key.
pub trait PublicKey {
    /// Returns the modulus of the key.
    fn n(&self) -> &BigUint;
    /// Returns the public exponent of the key.
    fn e(&self) -> u32;
    /// Returns the modulus size in bytes. Raw signatures and ciphertexts for
    /// or by this public key will have the same size.
    fn size(&self) -> usize {
        (self.n().bits() + 7) / 8
    }
}

impl PublicKey for RSAPublicKey {
    fn n(&self) -> &BigUint {
        &self.n
    }

    fn e(&self) -> u32 {
        self.e
    }
}

impl RSAPublicKey {
    /// Encrypt the given message.
    pub fn encrypt<R: Rng>(
        &self,
        rng: &mut R,
        padding: PaddingScheme,
        msg: &[u8],
    ) -> Result<Vec<u8>> {
        match padding {
            PaddingScheme::PKCS1v15 => pkcs1v15::encrypt(rng, self, msg),
            PaddingScheme::OAEP => unimplemented!("not yet implemented"),
        }
    }
}

impl<'a> PublicKey for &'a RSAPublicKey {
    fn n(&self) -> &BigUint {
        &self.n
    }

    fn e(&self) -> u32 {
        self.e
    }
}

impl PublicKey for RSAPrivateKey {
    fn n(&self) -> &BigUint {
        &self.n
    }

    fn e(&self) -> u32 {
        self.e
    }
}

impl<'a> PublicKey for &'a RSAPrivateKey {
    fn n(&self) -> &BigUint {
        &self.n
    }

    fn e(&self) -> u32 {
        self.e
    }
}

impl RSAPrivateKey {
    /// Generate a new RSA key pair of the given bit size using the passed in `rng`.
    pub fn new<R: Rng>(rng: &mut R, bit_size: usize) -> Result<RSAPrivateKey> {
        generate_multi_prime_key(rng, 2, bit_size)
    }

    /// Constructs an RSA key pair from the individual components.
    pub fn from_components(n: BigUint, e: u32, d: BigUint, primes: Vec<BigUint>) -> RSAPrivateKey {
        RSAPrivateKey { n, e, d, primes }
    }

    /// Returns the private exponent of the key.
    pub fn d(&self) -> &BigUint {
        &self.d
    }

    /// Returns the prime factors.
    pub fn primes(&self) -> &[BigUint] {
        &self.primes
    }

    /// Performs basic sanity checks on the key.
    /// Returns `Ok(())` if everything is good, otherwise an approriate error.
    pub fn validate(&self) -> Result<()> {
        check_public(self)?;

        Ok(())
    }

    /// Decrypt the given message.
    pub fn decrypt<R: Rng>(
        &self,
        rng: Option<&mut R>,
        padding: PaddingScheme,
        ciphertext: &[u8],
    ) -> Result<Vec<u8>> {
        match padding {
            PaddingScheme::PKCS1v15 => pkcs1v15::decrypt(rng, self, ciphertext),
            PaddingScheme::OAEP => unimplemented!("not yet implemented"),
        }
    }
}

#[inline]
pub fn check_public(public_key: &impl PublicKey) -> Result<()> {
    if public_key.e() < 2 {
        return Err(format_err!("public exponent too small"));
    }

    if public_key.e() > 1 << (31 - 1) {
        return Err(format_err!("public exponent too large"));
    }

    Ok(())
}

#[inline]
pub fn encrypt<K: PublicKey>(key: &K, m: &BigUint) -> BigUint {
    let e = BigUint::from_u64(u64::from(key.e())).unwrap();
    m.modpow(&e, key.n())
}

/// Performs RSA decryption, resulting in a plaintext `BigUint`.
/// Peforms RSA blinding if an `Rng` is passed.
#[inline]
pub fn decrypt<R: Rng>(
    mut rng: Option<&mut R>,
    priv_key: &RSAPrivateKey,
    c: &BigUint,
) -> Result<BigUint> {
    if c > priv_key.n() {
        return Err(format_err!("decryption error"));
    }

    if priv_key.n().is_zero() {
        return Err(format_err!("decryption error"));
    }

    let mut c = c.clone();
    let mut ir = None;

    if let Some(ref mut rng) = rng {
        // Blinding enabled. Blinding involves multiplying c by r^e.
        // Then the decryption operation performs (m^e * r^e)^d mod n
        // which equals mr mod n. The factor of r can then be removed
        // by multiplying by the multiplicative inverse of r.

        let mut r: BigUint;
        loop {
            r = rng.gen_biguint_below(priv_key.n());
            if r.is_zero() {
                r = BigUint::one();
            }
            ir = r.clone().mod_inverse(priv_key.n());
            if ir.is_some() {
                break;
            }
        }

        let e = BigUint::from_u64(u64::from(priv_key.e())).unwrap();
        let rpowe = r.modpow(&e, priv_key.n()); // N != 0
        c = (c * &rpowe) % priv_key.n();
    }

    // TODO: use precomputed once implemented
    let m = c.modpow(priv_key.d(), priv_key.n());

    match ir {
        Some(ref ir) => {
            // unblind
            Ok((m * ir) % priv_key.n())
        }
        None => Ok(m),
    }
}

/// Returns a new vector of the given length, with 0s left padded.
#[inline]
pub fn left_pad(input: &[u8], size: usize) -> Vec<u8> {
    let n = if input.len() > size {
        size
    } else {
        input.len()
    };

    let mut out = vec![0u8; size];
    out[size - n..].copy_from_slice(input);
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_traits::{FromPrimitive, ToPrimitive};
    use rand::{thread_rng, ThreadRng};

    #[test]
    fn test_from_into() {
        let private_key = RSAPrivateKey {
            n: BigUint::from_u64(100).unwrap(),
            e: 200,
            d: BigUint::from_u64(123).unwrap(),
            primes: vec![],
        };
        let public_key: RSAPublicKey = private_key.into();

        assert_eq!(public_key.n().to_u64(), Some(100));
        assert_eq!(public_key.e(), 200);
    }

    fn test_key_basics(private_key: RSAPrivateKey) {
        private_key.validate().expect("failed to validate");

        assert!(
            private_key.d() < private_key.n(),
            "private exponent too large"
        );

        let pub_key: RSAPublicKey = private_key.clone().into();
        let m = BigUint::from_u64(42).unwrap();
        let c = encrypt(&pub_key, &m);
        let m2 = decrypt::<ThreadRng>(None, &private_key, &c).unwrap();
        assert_eq!(m, m2);
        let mut rng = thread_rng();
        let m3 = decrypt(Some(&mut rng), &private_key, &c).unwrap();
        assert_eq!(m, m3);
    }

    macro_rules! key_generation {
        ($name:ident, $multi:expr, $size:expr) => {
            #[test]
            fn $name() {
                let mut rng = thread_rng();
                let private_key = if $multi == 2 {
                    RSAPrivateKey::new(&mut rng, $size).unwrap()
                } else {
                    generate_multi_prime_key(&mut rng, $multi, $size).unwrap()
                };
                assert_eq!(private_key.n().bits(), $size);

                test_key_basics(private_key);
            }
        };
    }

    key_generation!(key_generation_128, 2, 128);
    key_generation!(key_generation_1024, 2, 1024);

    key_generation!(key_generation_multi_3_256, 3, 256);

    key_generation!(key_generation_multi_4_64, 4, 64);

    key_generation!(key_generation_multi_5_64, 5, 64);
    key_generation!(key_generation_multi_8_576, 8, 576);
    key_generation!(key_generation_multi_16_1024, 16, 1024);

    #[test]
    fn test_impossible_keys() {
        // make sure not infinite loops are hit here.
        let mut rng = thread_rng();
        for i in 0..32 {
            let _ = RSAPrivateKey::new(&mut rng, i).is_err();
            let _ = generate_multi_prime_key(&mut rng, 3, i);
            let _ = generate_multi_prime_key(&mut rng, 4, i);
            let _ = generate_multi_prime_key(&mut rng, 5, i);
        }
    }
}
