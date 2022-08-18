use borsh::BorshSerialize;
use libzeropool::fawkes_crypto::{ff_uint::Num, engines::bn256::Fr};
use libzeropool::fawkes_crypto::{engines::bn256::Fq, backend::bellman_groth16::{prover::Proof, engines::Bn256}};
use num_bigint::{BigInt, ToBigInt};


pub fn serialize(num: Num<Fr>) -> Result<[u8; 32], std::io::Error> {

    let mut buf: [u8; 32] = [0; 32];

    BorshSerialize::serialize(&num, &mut &mut buf[0..32]).unwrap();

    buf.reverse();

    Ok(buf)
}


pub trait BytesRepr {
    fn bytes(&self) -> Vec<u8> 
    where 
        Self: BorshSerialize
    {
        self.bytes_padded(32)
    }

    fn bytes_padded(&self, len: usize) -> Vec<u8>
    where 
        Self: BorshSerialize
    {
        let mut buf = self.try_to_vec().unwrap();
        buf.resize(len, 0);
        buf.reverse();
        buf
    }
}

impl BytesRepr for Num<Fr> {}

impl BytesRepr for Num<Fq> {}

impl BytesRepr for u64 {}

impl BytesRepr for i64 {
    fn bytes_padded(&self, len: usize) -> Vec<u8> {
        if *self < 0 {
            (BigInt::from(2_i32).pow( 8 * len as u32) + self.to_bigint().unwrap()).to_bytes_be().1
        } else {
            (*self as u64).bytes_padded(len)
        }
    }
}

impl BytesRepr for Proof<Bn256> {
    fn bytes(&self) -> Vec<u8> {
        vec![self.a.0, self.a.1, self.b.0.0, self.b.0.1, self.b.1.0, self.b.1.1, self.c.0, self.c.1] // TODO: fix it
            .into_iter()
            .map(|num| num.bytes())
            .collect::<Vec<_>>()
            .concat()
    }

    fn bytes_padded(&self, _: usize) -> Vec<u8> {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    use libzeropool::fawkes_crypto::{ff_uint::{NumRepr, Num}, engines::{U256, bn256::Fr}};

    use crate::helpers::BytesRepr;

    #[test]
    fn test_num_fr_bytes() {
        let num: Num<Fr> = Num::from_uint(NumRepr(U256::from("11094997625971279522840469091548993338510923193332467128787568689747440336167"))).unwrap();
        assert_eq!("18878bce5c7454f688e229029c85b06b82480d33ccd72b4cf15fab8cab027d27", hex::encode(num.bytes_padded(32)));
    }

    #[test]
    fn test_u64_bytes() {
        assert_eq!("00ee", hex::encode(238_u64.bytes_padded(2)));
    }

    #[test]
    fn test_negative_i64_bytes() {
        assert_eq!("ffffffffc3cc9f80", hex::encode((-1010000000 as i64).bytes_padded(8)));
    }

    #[test]
    fn test_positive_i64_bytes() {
        assert_eq!("0000000000000080", hex::encode((128 as i64).bytes_padded(8)));
    }
}