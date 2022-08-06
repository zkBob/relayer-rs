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


pub trait HexRepr {
    fn to_hex(&self) -> String 
    where 
        Self: BorshSerialize
    {
        self.to_hex_padded(32)
    }

    fn to_hex_padded(&self, len: usize) -> String 
    where 
        Self: BorshSerialize
    {
        let mut buf = self.try_to_vec().unwrap();
        buf.resize(len, 0);
        buf.reverse();
        hex::encode(buf)
    }
}

impl HexRepr for Num<Fr> {}

impl HexRepr for Num<Fq> {}

impl HexRepr for u64 {}

impl HexRepr for i64 {
    fn to_hex_padded(&self, len: usize) -> String {
        if *self < 0 {
            let buf = (BigInt::from(2_i32).pow( 8 * len as u32) + self.to_bigint().unwrap()).to_bytes_be().1;
            hex::encode(buf)
        } else {
            (*self as u64).to_hex_padded(len)
        }
    }
}

impl HexRepr for Proof<Bn256> {
    fn to_hex(&self) -> String {
        vec![self.a.0, self.a.1, self.b.0.0, self.b.0.1, self.b.1.0, self.b.1.1, self.c.0, self.c.1] // TODO: fix it
            .into_iter()
            .map(|num| num.to_hex())
            .collect::<Vec<_>>()
            .join("")
    }

    fn to_hex_padded(&self, _: usize) -> String {
        unimplemented!();
    }
}

#[cfg(test)]
mod tests {
    use libzeropool::{fawkes_crypto::{ff_uint::{NumRepr, Num}, engines::{U256, bn256::Fr}}};

    use crate::helpers::HexRepr;

    #[test]
    fn test_num_fr_to_hex() {
        let num: Num<Fr> = Num::from_uint(NumRepr(U256::from("11094997625971279522840469091548993338510923193332467128787568689747440336167"))).unwrap();
        assert_eq!("18878bce5c7454f688e229029c85b06b82480d33ccd72b4cf15fab8cab027d27", num.to_hex_padded(32));
    }

    #[test]
    fn test_u64_to_hex() {
        assert_eq!("00ee", 238_u64.to_hex_padded(2));
    }

    #[test]
    fn test_negative_i64_to_hex() {
        assert_eq!("ffffffffc3cc9f80", (-1010000000 as i64).to_hex_padded(8));
    }

    #[test]
    fn test_positive_i64_to_hex() {
        assert_eq!("0000000000000080", (128 as i64).to_hex_padded(8));
    }
}