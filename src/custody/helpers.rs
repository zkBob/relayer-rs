use libzeropool::fawkes_crypto::ff_uint::Num;

use super::types::Fr;

pub trait AsU64Amount {
    fn as_u64_amount(&self) -> u64;
}

// It is applicable to tx amount only because tx amount is exactly 64 bit
impl AsU64Amount for Num<Fr> {
    fn as_u64_amount(&self) -> u64 {
        self.to_uint().0.0[0]
    }
}