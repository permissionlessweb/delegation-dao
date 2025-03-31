use std::{fs::File, str::FromStr};

use clap::Parser;
use cosmos_sdk_proto::cosmos::base::query::v1beta1::PageRequest;
use cosmwasm_std::Uint128;
use csv::Reader;
use cw_orch::{
    daemon::{queriers::Staking, DaemonBuilder, TxSender},
    prelude::*,
};

use tokio::runtime::Runtime;

pub const NEW_DELS_FILE: &str = "./data/new-delegations.csv";

pub const BITSONG_NETWORK: NetworkInfo = NetworkInfo {
    chain_name: "Bitsong",
    pub_address_prefix: "bitsong",
    coin_type: 639u32,
};

pub const BITSONG_MAINNET: ChainInfo = ChainInfo {
    kind: ChainKind::Mainnet,
    chain_id: "bitsong-2b",
    gas_denom: "ubtsg",
    gas_price: 0.025,
    grpc_urls: &["http://bitsong-grpc.polkachu.com:16090"],
    network_info: BITSONG_NETWORK,
    lcd_url: None,
    fcd_url: None,
};

// todo: move to .env file
pub const MNEMONIC: &str =
        "garage dial step tourist hint select patient eternal lesson raccoon shaft palace flee purpose vivid spend place year file life cliff winter race fox";

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Network to deploy on: main, testnet, local
    #[clap(short, long)]
    network: String,
    // /// Path to the script library
    // current_directory: String,
}
fn main() -> anyhow::Result<()> {
    // parse cargo command arguments for network type
    let args = Args::parse();

    let delegation_dao_addrs: Vec<String> = [
        "bitsong166d42nyufxrh3jps5wx3egdkmvvg7jl6k33yut".into(),
        "bitsong1nphhydjshzjevd03afzlce0xnlrnsm27hy9hgd".into(),
        "bitsong1tgzday8yewn8n5j0prgsc9t5r3gg2cwnyf9jlv".into(),
    ]
    .to_vec();

    println!("Running Bitsong Delegation Realignment Protocol...");
    let bitsong_chain: ChainInfoOwned = match args.network.as_str() {
        "main" => BITSONG_MAINNET.to_owned(),
        // "testnet" => BITSONG_TESTNET.to_owned(),
        // "local" => LOCAL_NETWORK1.to_owned(),
        _ => panic!("Invalid network"),
    }
    .into();

    let chain = DaemonBuilder::new(bitsong_chain.clone())
        .mnemonic(MNEMONIC)
        .build()?;

    let staking_query_client: Staking = chain.querier();

    // Create a new runtime for async execution
    let rt = Runtime::new()?;

    // Execute the async function using the runtime
    if let Err(err) = rt.block_on(realign_delegations(
        staking_query_client,
        delegation_dao_addrs,
        chain.node_querier().latest_block()?.height,
    )) {
        log::error!("{}", err);
        err.chain()
            .skip(1)
            .for_each(|cause| log::error!("because: {}", cause));

        ::std::process::exit(1);
    }
    Ok(())
}

async fn realign_delegations(
    staking_query_client: Staking,
    dao_addrs: Vec<String>,
    height: u64,
) -> anyhow::Result<()> {
    // logs any errors
    env_logger::init();
    // Get unbonded validators
    let unbonded_vals = staking_query_client
        ._validators(queriers::StakingBondStatus::Unbonded)
        .await?;
    let unbonding_vals = staking_query_client
        ._validators(queriers::StakingBondStatus::Unbonding)
        .await?;

    let val_historical = staking_query_client
        ._historical_info(height.try_into().unwrap())
        .await?;

    // Process each DAO address
    for dao in dao_addrs {
        println!("\nProcessing DAO address: {}", dao);

        // Get all delegations for the current DAO address
        let mut next_key = None;
        let mut all_delegations = Vec::new();
        loop {
            let response = staking_query_client
                ._delegator_delegations(&Addr::unchecked(&dao), next_key)
                .await?;

            all_delegations.extend(response.delegation_responses);

            // Check if there's more pages to fetch
            match response.pagination {
                Some(pagination) => {
                    if pagination.next_key.is_empty() {
                        break;
                    }

                    next_key = Some(PageRequest {
                        key: pagination.next_key,
                        offset: 0,
                        limit: 100,
                        count_total: false,
                        reverse: false,
                    });
                }
                None => {
                    // No more pages
                    break;
                }
            }
        }

        println!("Found {} existing delegations", all_delegations.len());

        // Load new delegations from CSV file
        let loaded = load_new_delegations(NEW_DELS_FILE);
        let aligned_vals = loaded.0;
        let total_to_del = Uint128::from(loaded.1).checked_mul(Uint128::new(10u128).pow(6))?;

        println!(
            "Loaded {} new delegations with total amount {}ubtsg",
            aligned_vals.len(),
            total_to_del
        );

        // Maps for tracking delegation changes
        // map of delegations to make, along with the total
        let mut del_map: (Vec<(String, Uint128)>, Uint128) = (vec![], Uint128::zero());
        // map of delegations to redelegate or remove
        let mut redel_map: (Vec<(String, Uint128)>, Uint128) = (vec![], Uint128::zero());

        // Process each existing delegation
        for delres in all_delegations.clone() {
            let metadata = delres.delegation.unwrap();
            let balance = Uint128::from_str(&delres.balance.unwrap().amount)
                .expect("Failed to parse balance");

            // Check if validator exists in our aligned validators list
            if let Some(exists) = aligned_vals
                .iter()
                .find(|a| a.0 == metadata.validator_address)
            {
                let new = Uint128::from(exists.1);
                if new < balance {
                    let diff = balance.checked_sub(new).expect("dang");
                    redel_map.0.push((metadata.validator_address.clone(), diff));
                    redel_map.1 += diff;
                    println!(
                        "Will reduce {}ubtsg from {}",
                        diff, metadata.validator_address
                    );
                } else if new > balance {
                    // We need to increase this delegation
                    let diff = new.checked_sub(balance).expect("darn");
                    del_map.0.push((metadata.validator_address.clone(), diff));
                    del_map.1 += diff;
                    println!("Will add {} to {}", diff, metadata.validator_address);
                } else {
                    // Keeping delegation as is
                    println!(
                        "Keeping {} for {} unchanged",
                        balance, metadata.validator_address
                    );
                }
            } else {
                // Validator not in aligned list, check if it's unbonded
                if unbonded_vals
                    .iter()
                    .any(|ub| ub.address == metadata.validator_address)
                {
                    // Remove delegations to unbonded validators
                    redel_map
                        .0
                        .push((metadata.validator_address.clone(), balance));
                    redel_map.1 += balance;
                    println!(
                        "Will remove {} from unbonded validator {}",
                        balance, metadata.validator_address
                    );
                } else if unbonding_vals
                    .iter()
                    .any(|ub| ub.address == metadata.validator_address)
                {
                    // Remove delegations to unbonded validators
                    redel_map
                        .0
                        .push((metadata.validator_address.clone(), balance));
                    redel_map.1 += balance;
                    println!(
                        "Will remove {} from unbonded validator {}",
                        balance, metadata.validator_address
                    );
                } else {
                    match val_historical.hist {
                        Some(ref hist) => {
                            if hist.valset.clone().into_iter().any(|e| {
                                e.operator_address == metadata.delegator_address && e.jailed
                            }) {
                                // Remove delegations to jailed validators
                                redel_map
                                    .0
                                    .push((metadata.validator_address.clone(), balance));
                                redel_map.1 += balance;
                            }
                        }
                        None => todo!(),
                    }
                }
            }
        }

        // Check for new validators we need to delegate to
        for (val_addr, amount) in &aligned_vals {
            if !all_delegations.iter().any(|d| {
                d.delegation
                    .as_ref()
                    .map_or(false, |del| del.validator_address == *val_addr)
            }) {
                // This is a completely new validator to delegate to
                let amount_uint = Uint128::from(*amount);
                del_map.0.push((val_addr.clone(), amount_uint));
                del_map.1 += amount_uint;
                println!("Will add new delegation of {} to {}", amount_uint, val_addr);
            }
        }

        // Print summary of delegation changes
        println!("\n--- DELEGATIONS TO ADD ---");
        println!("Total to delegate: {}", del_map.1);
        for (validator, amount) in &del_map.0 {
            println!("  {} → {}", validator, amount);
        }

        println!("\n--- DELEGATIONS TO REMOVE ---");
        println!("Total to redelegate: {}", redel_map.1);
        for (validator, amount) in &redel_map.0 {
            println!("  {} → {}", validator, amount);
        }

        // Validate that the total to delegate matches what we parsed
        if del_map.1 != total_to_del {
            println!(
                "WARNING: Total to delegate ({}) doesn't match loaded total ({})",
                del_map.1, total_to_del
            );
        }
    }

    Ok(())
}

// Loads array of validators getting new delegations from file, returning the total new delegations
fn load_new_delegations(fp: &str) -> (Vec<(String, u64)>, u64) {
    let csv = File::open(fp).expect("Could not open file");
    let mut reader = Reader::from_reader(csv);
    let mut aligned_vals = vec![];
    let mut total = 0;

    for result in reader.records() {
        match result {
            Ok(record) => {
                let addr: String = record[0].parse().expect("Could not parse first value");
                let value2: u64 = record[1].parse().expect("Could not parse second value");
                aligned_vals.push((addr, value2));
                total += value2;
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
                continue;
            }
        }
    }
    (aligned_vals, total)
}
