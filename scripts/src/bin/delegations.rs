use std::{fs::File, io::Write, str::FromStr};

use clap::Parser;
use cosmos_sdk_proto::cosmos::{
    base::query::v1beta1::PageRequest,
    base::v1beta1::Coin as ProtoCoin,
    staking::v1beta1::{MsgBeginRedelegate, MsgDelegate},
};
use cosmwasm_std::{Decimal, Uint128};
use csv::Reader;
use cw_orch::{
    environment::{NetworkInfo,ChainKind},
    daemon::{queriers::Staking, DaemonBuilder, TxSender},
    prelude::*,
};
use serde::Serialize;
use tokio::runtime::Runtime;

pub const NEW_DELS_FILE: &str = "./src/bin/data/new-delegations.csv";

#[derive(Serialize)]
struct RedelegateMsg {
    delegator_address: String,
    validator_src_address: String,
    validator_dst_address: String,
    amount: String,
    denom: String,
}

#[derive(Serialize)]
struct DelegateMsg {
    delegator_address: String,
    validator_address: String,
    amount: String,
    denom: String,
}

/// serializes the raw maps we collect to determine how to handle redelegations
#[derive(Serialize)]
struct RawMapsLogs {
    redelegations: Vec<(String, Uint128)>,
    delegations: Vec<(String, Uint128)>,
}

#[derive(Serialize)]
struct MessageExport {
    redelegations: Vec<RedelegateMsg>,
    delegations: Vec<DelegateMsg>,
}

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

    let bitsong_chain: ChainInfoOwned = match args.network.as_str() {
        "main" => BITSONG_MAINNET.to_owned(),
        // "testnet" => BITSONG_TESTNET.to_owned(),
        // "local" => LOCAL_NETWORK1.to_owned(),
        _ => panic!("Invalid network"),
    }
    .into();

    // connect to chain with mnemonic
    let chain = DaemonBuilder::new(bitsong_chain.clone())
        .mnemonic(MNEMONIC)
        .build()?;

    // create client
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

    // Load new delegations from CSV file
    let new_delegations = load_new_delegations(NEW_DELS_FILE);
    let aligned_vals = new_delegations.0;
    // Calculated total amount to have delegated (includes current delegations )
    let total_to_del = Uint128::from(new_delegations.1).checked_mul(Uint128::new(10).pow(6))?;

    println!("Running Bitsong Delegation Realignment Protocol...");
    println!("{} dels with {}ubtsg", aligned_vals.len(), total_to_del);

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

        // Maps for tracking delegation changes
        // del_map: map of delegations to make to fufill this rounds obligations, along with the total amount need to delegate.
        // redel_map: map of delegations to redelegate or remove
        let mut del_map: (Vec<(String, Uint128)>, Uint128) = (vec![], Uint128::zero());
        let mut redel_map: (Vec<(String, Uint128)>, Uint128) = (vec![], Uint128::zero());

        for delres in all_delegations.clone() {
            let metadata = delres.delegation.unwrap();
            let balance = Uint128::from_str(&delres.balance.unwrap().amount)
                .expect("Failed to parse balance");

            if !balance.is_zero() {
                // Check if validator exists in our aligned validators list
                if let Some(exists) = aligned_vals
                    .iter()
                    .find(|a| a.0 == metadata.validator_address)
                {
                    // check if this address is adding or removing delegations
                    let new = Uint128::from(exists.1);
                    if new < balance {
                        // removing
                        let diff = balance.checked_sub(new).expect("dang");
                        let dd = Decimal::from_atomics(diff, 6)?;
                        redel_map.0.push((metadata.validator_address.clone(), diff));
                        redel_map.1 += diff;
                        println!(
                            "Will reduce {}ubtsg from {}",
                            dd, metadata.validator_address
                        );
                    } else if new > balance {
                        // adding
                        let diff = new.checked_sub(balance).expect("darn");
                        del_map.0.push((metadata.validator_address.clone(), diff));
                        del_map.1 += diff;
                        println!("Will add {}ubtsg to {}", diff, metadata.validator_address);
                    } else {
                        // Keeping delegation as is
                        println!(
                            "Keeping {} for {} unchanged",
                            balance, metadata.validator_address
                        );
                    }
                } else {
                    // Validator not in aligned list, we are removing from
                    if unbonded_vals
                        .iter()
                        .any(|ub| ub.address == metadata.validator_address)
                    {
                        // Remove delegations to unbonded validators

                        let dd = Decimal::from_atomics(balance, 6)?;
                        println!(
                            "Disregarding  unbonded validator {} with {}ubtsg",
                            dd, metadata.validator_address
                        );
                    } else if unbonding_vals
                        .iter()
                        .any(|ub| ub.address == metadata.validator_address)
                    {
                        if balance.u128() != 0u128 {
                            // Remove delegations to unbonded validators
                            redel_map
                                .0
                                .push((metadata.validator_address.clone(), balance));
                            redel_map.1 += balance;
                            println!(
                                "Will remove {}ubtsg from unbonding validator {}",
                                balance, metadata.validator_address
                            );
                        }
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
                            None => unimplemented!(),
                        }
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
                let ant_uint = Uint128::from(*amount);
                let amnt_dec = Decimal::from_atomics(ant_uint, 0)?;
                del_map.0.push((val_addr.clone(), ant_uint));
                del_map.1 += ant_uint;
                println!(
                    "Will add new delegation of {}ubtsg to {}",
                    amnt_dec, val_addr
                );
            }
        }

        // Validate that the total to delegate matches what we parsed
        if del_map.1 != total_to_del {
            println!(
                "WARNING: Total to delegate ({}) doesn't match loaded total ({})",
                del_map.1, total_to_del
            );
        }

        // Sort del_map by amount in ascending order
        del_map.0.sort_by(|a, b| a.1.cmp(&b.1));
        // Sort redel_map by amount in decending order
        redel_map.0.sort_by(|a, b| b.1.cmp(&a.1));
        println!("sorted");

        // for each object in redel_map, try to use the tokens to redelgate & satisfy an obligated delegation in del_map.
        // If we are able to satissfy a new delegation with an oold delegation, continue attempting to satisfy delegations to make with the redelegation value, until it is 0.
        // once zero, go to the next object in the redel list and continue this process until there are no more redelegations to consume.
        // If there are no more redelegations to consume but still delegations to create, we need to form normally

        // Generate redelegation and delegation messages
        let mut redelegation_msgs = Vec::<MsgBeginRedelegate>::new();
        let mut delegation_msgs = Vec::<MsgDelegate>::new();
        let mut to_delegate = del_map.0.clone();
        let mut to_redelegate = redel_map.0.clone();
        let denom = "ubtsg";

        // For each redelegation to process
        // Process each delegation by potentially using multiple redelegations
        while !to_delegate.is_empty() && !to_redelegate.is_empty() {
            // Get the current delegation to satisfy (using the smallest one first)
            let (dst_validator, mut del_amount) = to_delegate[0].clone();
            to_delegate.remove(0);

            // Keep trying to satisfy this delegation until it's fully satisfied or no more redelegations
            while !del_amount.is_zero() && !to_redelegate.is_empty() {
                // Get the current redelegation (using the largest one first)
                let (src_validator, redel_amount) = to_redelegate[0].clone();

                if redel_amount >= del_amount {
                    // We can fully satisfy this delegation with the current redelegation
                    redelegation_msgs.push(MsgBeginRedelegate {
                        delegator_address: dao.clone(),
                        validator_src_address: src_validator.clone(),
                        validator_dst_address: dst_validator.clone(),
                        amount: Some(ProtoCoin {
                            denom: denom.to_string(),
                            amount: del_amount.to_string(),
                        }),
                    });
                    // Update the remaining redelegation amount
                    let remaining = redel_amount.checked_sub(del_amount)?;
                    if remaining.is_zero() {
                        // This redelegation is completely consumed
                        to_redelegate.remove(0);
                    } else {
                        // Update with the remaining amount
                        to_redelegate[0].1 = remaining;
                    }
                    // Delegation is fully satisfied
                    del_amount = Uint128::zero();
                } else {
                    // We can only partially satisfy this delegation with current redelegation.
                    redelegation_msgs.push(MsgBeginRedelegate {
                        delegator_address: dao.clone(),
                        validator_src_address: src_validator.clone(),
                        validator_dst_address: dst_validator.clone(),
                        amount: Some(ProtoCoin {
                            denom: denom.to_string(),
                            amount: redel_amount.to_string(),
                        }),
                    });

                    // Reduce the remaining delegation amount
                    del_amount = del_amount.checked_sub(redel_amount).unwrap();
                    println!("del_amount: {}", del_amount);
                    // completely consumed, remove it
                    to_redelegate.remove(0);
                }

                // If we still have delegation amount that couldn't be satisfied, add it back
                if !del_amount.is_zero() {
                    // We ran out of redelegations, so this will be a regular delegation
                    delegation_msgs.push(MsgDelegate {
                        delegator_address: dao.clone(),
                        validator_address: dst_validator.clone(),
                        amount: Some(ProtoCoin {
                            denom: denom.to_string(),
                            amount: del_amount.to_string(),
                        }),
                    });
                }
            }
        }

        // Print any remaining redelegations that couldn't be used
        for (src_validator, amount) in to_redelegate {
            println!(
                "Warning: {} ubtsg from {} couldn't be redelegated",
                amount, src_validator
            );
        }

        // Create normal delegation messages for any remaining delegations
        for (dst_validator, amount) in to_delegate {
            delegation_msgs.push(MsgDelegate {
                delegator_address: dao.clone(),
                validator_address: dst_validator,
                amount: Some(ProtoCoin {
                    denom: denom.to_string(),
                    amount: amount.to_string(),
                }),
            });
        }

        let total = del_map.1.checked_mul(Uint128::new(10u128).pow(6))?;
        // Print summary of delegation changes
        println!("\n--- DELEGATIONS TO ADD ---");
        println!("Total to delegate: {}", total);
        for (validator, amount) in &del_map.0 {
            let amnt = Decimal::from_atomics(*amount, 6)?;
            println!("  {} → {}", validator, amnt);
        }

        println!("\n--- DELEGATIONS TO REMOVE ---");
        println!("Total to redelegate: {}", redel_map.1);
        for (validator, amount) in &redel_map.0 {
            let amnt = Decimal::from_atomics(*amount, 6)?;
            println!("  {} → {}", validator, amnt);
        }

        // Create the export object
        let export = MessageExport {
            redelegations: redelegation_msgs
                .iter()
                .map(|msg| {
                    let amount = msg.amount.as_ref().unwrap();
                    RedelegateMsg {
                        delegator_address: msg.delegator_address.clone(),
                        validator_src_address: msg.validator_src_address.clone(),
                        validator_dst_address: msg.validator_dst_address.clone(),
                        amount: amount.amount.clone(),
                        denom: amount.denom.clone(),
                    }
                })
                .collect(),
            delegations: delegation_msgs
                .iter()
                .map(|msg| {
                    let amount = msg.amount.as_ref().unwrap();
                    DelegateMsg {
                        delegator_address: msg.delegator_address.clone(),
                        validator_address: msg.validator_address.clone(),
                        amount: amount.amount.clone(),
                        denom: amount.denom.clone(),
                    }
                })
                .collect(),
        };
        // Serialize to JSON
        let json =
            serde_json::to_string_pretty(&export).expect("Failed to serialize messages to JSON");
        serialize_and_print(json, format!("delegation_messages_{}.json", dao))
    }

    Ok(())
}
fn serialize_and_print(json: String, filepath: String) {
    let mut file = File::create(filepath).expect("Failed to create JSON file");
    file.write_all(json.as_bytes())
        .expect("Failed to write JSON to file");
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
