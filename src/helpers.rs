use libzeropool::fawkes_crypto::{ff_uint::Num, engines::bn256::Fr};

pub fn serialize(num: Num<Fr>) -> Result<[u8; 32], std::io::Error> {
    use borsh::BorshSerialize;

    let mut buf: [u8; 32] = [0; 32];

    BorshSerialize::serialize(&num, &mut &mut buf[0..32]).unwrap();

    buf.reverse();

    Ok(buf)
}