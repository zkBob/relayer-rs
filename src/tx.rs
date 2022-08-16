use actix_web::web::Data;
use kvdb::KeyValueDB;
use libzeropool::{
    constants::{HEIGHT, OUTPLUSONELOG},
    fawkes_crypto::{
        backend::bellman_groth16::{engines::Bn256, prover::Proof, Parameters},
        engines::bn256::Fr,
        ff_uint::Num,
    },
    native::{
        params::PoolBN256,
        tree::{TreePub, TreeSec},
        tx,
    },
    POOL_PARAMS,
};
use libzkbob_rs::{merkle::MerkleTree, proof};

use crate::{helpers::HexRepr, state::Job};

const TRANSFER_INDEX_SIZE: usize = 6;
const ENERGY_SIZE: usize = 14;
const TOKEN_SIZE: usize = 8;

pub fn build<D: 'static + KeyValueDB>(
    job: &Data<Job>,
    tree: &MerkleTree<D, PoolBN256>,
    params: &Parameters<Bn256>,
) -> String {
    let tx_request = &job.transaction_request.as_ref().unwrap();
    let tree_proof = prove_tree(&job, tree, params);

    let nullifier = tx_request.proof.inputs[1];
    let out_commit = tree_proof.0[2];

    let delta_params = tx::parse_delta(tx_request.proof.inputs[3]);
    let token_amount: i64 = delta_params.0.try_into().unwrap();
    let energy_amount: i64 = delta_params.1.try_into().unwrap();
    let transfer_index: u64 = delta_params.2.try_into().unwrap();

    let root_after = tree_proof.0[1];
    let tx_type = tx_request.tx_type.clone();
    let memo = tx_request.memo.clone();
    let memo_size = (memo.len() as u64 / 2).to_hex_padded(2);

    let mut tx_data = vec![
        nullifier.to_hex(),
        out_commit.to_hex(),
        transfer_index.to_hex_padded(TRANSFER_INDEX_SIZE),
        energy_amount.to_hex_padded(ENERGY_SIZE),
        token_amount.to_hex_padded(TOKEN_SIZE),
        tx_request.proof.proof.to_hex(),
        root_after.to_hex(),
        tree_proof.1.to_hex(),
        tx_type,
        memo_size,
        memo,
    ];

    let deposit_signature = tx_request.deposit_signature.clone();
    if !deposit_signature.is_empty() {
        tx_data.push(deposit_signature.replace("0x", ""))
    }

    let tx_data = tx_data.join("");
    tracing::debug!("[Job: {}] TX DATA: {}", job.id, tx_data);
    tx_data
}

fn prove_tree<D: 'static + KeyValueDB>(
    job: &Data<Job>,
    tree: &MerkleTree<D, PoolBN256>,
    params: &Parameters<Bn256>,
) -> (Vec<Num<Fr>>, Proof<Bn256>) {
    let out_commit = job.transaction_request.as_ref().unwrap().proof.inputs[2];

    let next_leaf_index = tree.next_index();

    let next_commit_index = next_leaf_index >> OUTPLUSONELOG;
    let prev_commit_index = next_commit_index - 1;

    let root_before = tree.get_root();
    let root_after = tree.get_root_after_virtual(vec![out_commit]);

    let proof_filled = tree.get_proof_unchecked::<{ HEIGHT - OUTPLUSONELOG }>(prev_commit_index);
    let proof_free = tree.get_proof_unchecked::<{ HEIGHT - OUTPLUSONELOG }>(next_commit_index);

    let leaf = out_commit;
    let prev_leaf = tree.get(OUTPLUSONELOG as u32, prev_commit_index);

    let tree_pub = TreePub {
        root_before,
        root_after,
        leaf,
    };

    let tree_sec = TreeSec {
        proof_filled,
        proof_free,
        prev_leaf,
    };

    tracing::debug!("[Job: {}] Proving tree update...", job.id);
    let tree_proof = proof::prove_tree(params, &POOL_PARAMS.clone(), tree_pub, tree_sec);
    tracing::debug!("[Job: {}] Tree update proved!", job.id);
    tree_proof
}
