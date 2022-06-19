#[test]
fn keccak_test() {

    use web3::signing::keccak256;
    use hex_literal::hex;

    let a = keccak256("Transfer(address,address,uint256)".as_bytes());

    assert_eq!(a, hex!("ddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef"))

}

#[test]
fn test_sync() {
    
}