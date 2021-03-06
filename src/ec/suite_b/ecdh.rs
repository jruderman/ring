// Copyright 2015-2016 Brian Smith.
//
// Permission to use, copy, modify, and/or distribute this software for any
// purpose with or without fee is hereby granted, provided that the above
// copyright notice and this permission notice appear in all copies.
//
// THE SOFTWARE IS PROVIDED "AS IS" AND THE AUTHORS DISCLAIM ALL WARRANTIES
// WITH REGARD TO THIS SOFTWARE INCLUDING ALL IMPLIED WARRANTIES OF
// MERCHANTABILITY AND FITNESS. IN NO EVENT SHALL THE AUTHORS BE LIABLE FOR ANY
// SPECIAL, DIRECT, INDIRECT, OR CONSEQUENTIAL DAMAGES OR ANY DAMAGES
// WHATSOEVER RESULTING FROM LOSS OF USE, DATA OR PROFITS, WHETHER IN AN ACTION
// OF CONTRACT, NEGLIGENCE OR OTHER TORTIOUS ACTION, ARISING OUT OF OR IN
// CONNECTION WITH THE USE OR PERFORMANCE OF THIS SOFTWARE.

//! ECDH key agreement using the P-256 and P-384 curves.

#![allow(unsafe_code)]

use {agreement, bssl, c, ec, rand};
use super::ops::*;
use super::public_key::*;
use untrusted;

/// A key agreement algorithm.
macro_rules! ecdh {
    ( $NAME:ident, $bits:expr, $name_str:expr, $group:expr, $nid:expr,
      $ecdh:ident, $generate_private_key:ident, $public_from_private:ident ) =>
    {
        #[doc="ECDH using the NSA Suite B"]
        #[doc=$name_str]
        #[doc="curve."]
        ///
        /// Public keys are encoding in uncompressed form using the
        /// Octet-String-to-Elliptic-Curve-Point algorithm in [SEC 1: Elliptic
        /// Curve Cryptography, Version 2.0](http://www.secg.org/sec1-v2.pdf).
        /// Public keys are validated during key agreement as described in
        /// [NIST Special Publication 800-56A, revision
        /// 2](http://csrc.nist.gov/groups/ST/toolkit/documents/SP800-56Arev1_3-8-07.pdf)
        /// Section 5.6.2.5 and the NSA's "Suite B Implementer's Guide to NIST
        /// SP 800-56A," Appendix B.3. Note that, as explained in the NSA
        /// guide, "partial" validation is equivalent to "full" validation for
        /// prime-order curves like this one.
        ///
        /// TODO: Each of the encoded coordinates are verified to be the
        /// correct length, but values of the allowed length that haven't been
        /// reduced modulo *q* are currently reduced mod *q* during
        /// verification. Soon, coordinates larger than *q* - 1 will be
        /// rejected.
        ///
        /// Only available in `use_heap` mode.
        pub static $NAME: agreement::Algorithm = agreement::Algorithm {
            i: ec::AgreementAlgorithmImpl {
                public_key_len: 1 + (2 * (($bits + 7) / 8)),
                elem_and_scalar_len: ($bits + 7) / 8,
                nid: $nid,
                generate_private_key: $generate_private_key,
                public_from_private: $public_from_private,
                ecdh: $ecdh,
            },
        };

        fn $ecdh(out: &mut [u8], my_private_key: &ec::PrivateKey,
                 peer_public_key: untrusted::Input) -> Result<(), ()> {
            ecdh($group, out, ($bits + 7) / 8, my_private_key, peer_public_key)
        }

        agreement_externs!($generate_private_key, $public_from_private);
    }
}

fn ecdh(group: &EC_GROUP, out: &mut [u8], elem_and_scalar_len: usize,
        my_private_key: &ec::PrivateKey, peer_public_key: untrusted::Input)
        -> Result<(), ()> {
    let (peer_x, peer_y) =
        try!(parse_uncompressed_point(peer_public_key, elem_and_scalar_len));
    bssl::map_result(unsafe {
        GFp_suite_b_ecdh(group, out.as_mut_ptr(), out.len(),
                         my_private_key.bytes.as_ptr(), elem_and_scalar_len,
                         peer_x.as_ptr(), peer_x.len(), peer_y.as_ptr(),
                         peer_y.len())
    })
}

ecdh!(ECDH_P256, 256, "P-256 (secp256r1)", &EC_GROUP_P256,
      415 /*NID_X9_62_prime256v1*/, p256_ecdh, GFp_p256_generate_private_key,
      GFp_p256_public_from_private);

ecdh!(ECDH_P384, 384, "P-384 (secp384r1)", &EC_GROUP_P384,
      715 /*NID_secp384r1*/, p384_ecdh, GFp_p384_generate_private_key,
      GFp_p384_public_from_private);

extern {
    fn GFp_suite_b_ecdh(group: &EC_GROUP, out: *mut u8,
                        out_len: c::size_t, private_key: *const u8,
                        private_key_len: c::size_t,
                        peer_public_key_x: *const u8,
                        peer_public_key_x_len: c::size_t,
                        peer_public_key_y: *const u8,
                        peer_public_key_y_len: c::size_t) -> c::int;
}


#[cfg(test)]
mod tests {
    use {agreement, rand};

    static SUPPORTED_SUITE_B_ALGS: [&'static agreement::Algorithm; 2] = [
        &agreement::ECDH_P256,
        &agreement::ECDH_P384,
    ];

    #[test]
    fn test_agreement_suite_b_ecdh_generate() {
        struct FixedByteRandom {
            byte: u8
        };

        impl rand::SecureRandom for FixedByteRandom {
            fn fill(&self, dest: &mut [u8]) -> Result<(), ()> {
                for d in dest {
                    *d = self.byte
                }
                Ok(())
            }
        }

        // Generates a string of bytes 0x00...00, which will always result in
        // a scalar value of zero.
        let random_00 = FixedByteRandom { byte: 0 };

        // Generates a string of bytes 0xFF...FF, which will be larger than the
        // group order of any curve that is supported.
        let random_ff = FixedByteRandom { byte: 0xff };

        for alg in SUPPORTED_SUITE_B_ALGS.iter() {
            // Test that the private key value zero is rejected and that
            // `generate` gives up after a while of only getting zeros.
            assert!(agreement::EphemeralPrivateKey::generate(alg, &random_00)
                        .is_err());

            // Test that the private key value larger than the group order is
            // rejected and that `generate` gives up after a while of only
            // getting values larger than the group order.
            assert!(agreement::EphemeralPrivateKey::generate(alg, &random_ff)
                        .is_err());

            // TODO XXX: Test that a private key value exactly equal to the
            // group order is rejected and that `generate` gives up after a
            // while of only getting that value from the PRNG. This is
            // non-trivial because it requires the test PRNG to generate a
            // series of bytes of output that, when interpreted as an array of
            // `BN_ULONG`s (which vary in size and endianness by platform), is
            // equal to the group order.
        }
    }
}
