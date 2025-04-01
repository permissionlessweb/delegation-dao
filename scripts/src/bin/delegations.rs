use std::{collections::HashMap, fs::File, io::Write, str::FromStr};

use clap::Parser;
use cosmos_sdk_proto::cosmos::{
    base::{query::v1beta1::PageRequest, v1beta1::Coin as ProtoCoin},
    staking::v1beta1::{
        DelegationResponse, MsgBeginRedelegate, MsgDelegate, MsgDelegateResponse, MsgUndelegate,
    },
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128};
use csv::Reader;
use cw_orch::{
    daemon::{queriers::Staking, DaemonBuilder, TxSender},
    environment::{ChainKind, NetworkInfo},
    prelude::*,
};
use serde::Serialize;
use tokio::runtime::Runtime;

pub const NEW_DELS_FILE: &str = "./src/bin/data/new-delegations.csv";

#[cw_serde]
struct AlignedValidator {
    operator_addr: String,
    current_delegations: Vec<Delegation>,
    new_delegation_amount: Uint128,
}
#[cw_serde]
struct Delegation {
    del_addr: String,
    operator_addr: String,
    amount: Uint128,
}

#[derive(Serialize)]
struct AllAlignedDelegations {
    delegations: Vec<Delegation>,
    total: Uint128,
}

#[derive(Serialize)]
struct RedelegateMsg {
    delegator_address: String,
    validator_src_address: String,
    validator_dst_address: String,
    amount: String,
    denom: String,
}
#[derive(Serialize)]
struct UndelegateMsg {
    delegator_address: String,
    validator_address: String,
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
    undelegates: Vec<UndelegateMsg>,
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
    let mut new_delegations_distribution = load_new_delegations(NEW_DELS_FILE);
    let mut new_delegations_validators = new_delegations_distribution.delegations;
    // Calculated total amount to have delegated (includes current delegations )
    let total_to_del = Uint128::from(new_delegations_distribution.total);

    println!("Running Bitsong Delegation Realignment Protocol...");
    println!(
        "{} dels with {}ubtsg",
        new_delegations_validators.len(),
        Decimal::from_atomics(total_to_del.checked_mul(1000000u128.into())?, 6)?
    );

    // collect  all dao delegations
    let mut all_delegations = Vec::new();
    for dao in &dao_addrs {
        println!("\nProcessing DAO address: {}", dao);
        let mut next_key = None;
        loop {
            let response = staking_query_client
                ._delegator_delegations(&Addr::unchecked(dao), next_key)
                .await?;

            all_delegations.extend(response.delegation_responses);
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
                    break;
                }
            }
        }
        println!("Found {} existing delegations", all_delegations.len());
    }

    // current_vals - array of validators and the DAOs delegations to them
    let mut current_vals: Vec<AlignedValidator> = Vec::new();

    for dels in all_delegations.clone() {
        let del = dels.delegation.unwrap();
        let balance =
            Uint128::from_str(&dels.balance.unwrap().amount).expect("Failed to parse balance");

        if !balance.is_zero() {
            // Check if validator exists in our aligned validators list

            if let Some(exists) = current_vals
                .iter_mut()
                .find(|a| a.operator_addr == del.validator_address)
            {
                exists.current_delegations.push(Delegation {
                    del_addr: del.delegator_address,
                    operator_addr: del.validator_address,
                    amount: balance,
                });
            } else {
                let target_amount = new_delegations_validators
                    .iter()
                    .find(|a| a.operator_addr == del.validator_address)
                    .map_or(Uint128::zero(), |a| a.amount);
                current_vals.push(AlignedValidator {
                    operator_addr: del.validator_address.clone(),
                    current_delegations: vec![Delegation {
                        del_addr: del.delegator_address,
                        operator_addr: del.validator_address.clone(),
                        amount: balance,
                    }],
                    new_delegation_amount: target_amount,
                });
            }
        }
    }

    // Add any completely new validators from aligned_vals that don't exist in current_vals
    for aligned_val in &new_delegations_validators {
        if !current_vals
            .iter()
            .any(|cv| cv.operator_addr == aligned_val.operator_addr)
        {
            current_vals.push(AlignedValidator {
                operator_addr: aligned_val.operator_addr.clone(),
                current_delegations: Vec::new(),
                new_delegation_amount: aligned_val.amount,
            });
        }
    }

    let mut all_redels: Vec<Delegation> = Vec::new();
    let mut all_dels: Vec<Delegation> = Vec::new();
    let mut del_total = Uint128::zero();
    let mut redel_total = Uint128::zero();

    for mut val in current_vals {
        println!("\nProcessing current_vals: {}", val.operator_addr);
        println!("dels count  : {}", val.current_delegations.len());
        // Calculate total current delegation to this validator
        let mut total_current_del = Uint128::zero();
        for cd in &val.current_delegations {
            total_current_del += cd.amount;
        }

        let is_unbonded = unbonded_vals.iter().any(|v| v.address == val.operator_addr);
        let is_unbonding = unbonding_vals
            .iter()
            .any(|v| v.address == val.operator_addr);
        let is_jailed = match &val_historical.hist {
            Some(hist) => hist
                .valset
                .iter()
                .any(|v| v.operator_address == val.operator_addr && v.jailed),
            None => false,
        };

        if val.new_delegation_amount.is_zero() || is_unbonded || is_unbonding || is_jailed {
            // If there are delegations to unbonded/unbonding/jailed validators, add to redelegation map
            if !total_current_del.is_zero() {
                all_redels.extend(val.current_delegations);
                redel_total += total_current_del;
                let reason = if is_unbonded {
                    "unbonded"
                } else if is_unbonding {
                    "unbonding"
                } else if is_jailed {
                    "jailed"
                } else {
                    "no-delegations"
                };
                println!(
                    "Will remove {}ubtsg from {} validator {}",
                    total_current_del, reason, val.operator_addr
                );
            }
            continue;
        } else if total_current_del > val.new_delegation_amount {
            // Need to reduce delegations
            let mut diff = total_current_del
                .checked_sub(val.new_delegation_amount)
                .expect("dang");
            // Sort current delegations by amount (largest first to optimize processing)
            let mut sorted_delegations = val.current_delegations.clone();
            sorted_delegations.sort_by(|a, b| b.amount.cmp(&a.amount));

            let mut remaining_diff = diff;
            let mut redelegations_to_add = Vec::new();

            for delegation in sorted_delegations {
                if remaining_diff.is_zero() {
                    break; // We've consumed all the difference
                }
                if delegation.amount <= remaining_diff {
                    // We can use this entire delegation
                    remaining_diff = remaining_diff
                        .checked_sub(delegation.amount)
                        .expect("subtraction overflow");
                    redelegations_to_add.push(delegation);
                } else {
                    // We need only part of this delegation
                    let mut partial_delegation = delegation.clone();
                    partial_delegation.amount = remaining_diff;
                    redelegations_to_add.push(partial_delegation);
                    remaining_diff = Uint128::zero();
                }
            }

            // Add the selected delegations to all_redels
            all_redels.extend(redelegations_to_add);

            // only set remaining difference to unbond.
        } else if total_current_del < val.new_delegation_amount {
            //  need to delegate
            let diff = val
                .new_delegation_amount
                .checked_sub(total_current_del)
                .expect("darn");

            let mut sorted_delegations = val.current_delegations.clone();
            sorted_delegations.sort_by(|a, b| b.amount.cmp(&a.amount));

            let mut remaining_diff = diff;
            let mut delegation_to_add = Vec::new();

            for delegation in sorted_delegations {
                if remaining_diff.is_zero() {
                    break; // We've consumed all the difference
                }

                if delegation.amount <= remaining_diff {
                    // We can use this entire delegation
                    remaining_diff = remaining_diff
                        .checked_sub(delegation.amount)
                        .expect("subtraction overflow");
                    delegation_to_add.push(delegation);
                } else {
                    // We need only part of this delegation
                    let mut partial_delegation = delegation.clone();
                    partial_delegation.amount = remaining_diff;
                    delegation_to_add.push(partial_delegation);
                    remaining_diff = Uint128::zero();
                }
            }

            // Add the selected delegations to all_redels
            all_dels.extend(delegation_to_add);
        }
    }

    //         // Sort del_map by amount in ascending order
    all_dels.sort_by(|a, b| a.amount.cmp(&b.amount));
    // Sort redel_map by amount in decending order
    all_redels.sort_by(|a, b| b.amount.cmp(&a.amount));
    println!("sorted");

    // Generate redelegation and delegation messages
    // In main processing function
    let (redelegation_msgs, delegation_msgs, undelegate_msgs) = optimize_delegations(
        all_delegations
            .iter()
            .map(|a| Delegation {
                del_addr: a.delegation.clone().expect("msg").delegator_address,
                operator_addr: a.delegation.clone().expect("msg").validator_address,
                amount: Uint128::from_str(a.balance.clone().expect("msg").amount.as_str()).unwrap(),
            })
            .collect(), // Current delegations
        &new_delegations_validators, // Target delegations
        "ubtsg",
    );

    let denom = "ubtsg";

    // 7 redelegations is max, so if we have any more than 7 redelegations to a validator, we going to need to satisfy the delegation obligations
    // if we have more redelegations than able to use, create max redelegate msg, and with remaining create both unbond and delegate.

    // let total = d.1.checked_mul(Uint128::new(10u128).pow(6))?;
    // Print summary of delegation changes
    println!("\n--- DELEGATIONS TO ADD ---");
    let mut total_del = Uint128::zero();
    for del in &delegation_msgs {
        let uint_amnt = Uint128::from_str(del.amount.clone().expect("shoot").amount.as_str())?;
        let amnt = Decimal::from_atomics(uint_amnt, 6)?;
        println!("  {} → {}BTSG", del.validator_address, amnt);
        total_del += uint_amnt;
    }
    println!(
        "Total to delegate: {}",
        Decimal::from_atomics(total_del, 6)?
    );
    println!("\n--- DELEGATIONS TO REMOVE ---");
    let mut total_redel = Uint128::zero();
    // println!("Total to redelegate: {}", redel_map.1);
    for redel in &redelegation_msgs {
        let uint_amnt = Uint128::from_str(redel.amount.clone().expect("shoot").amount.as_str())?;
        let amnt = Decimal::from_atomics(uint_amnt, 6)?;
        println!(
            "{} → {} → {}",
            redel.validator_src_address, amnt, redel.validator_dst_address
        );
        total_redel += uint_amnt;
    }
    println!(
        "Total to redelegate: {}BTSG",
        Decimal::from_atomics(total_redel, 6)?
    );

    let mut total_undel = Uint128::zero();
    for undel in &undelegate_msgs {
        let uint_amnt = Uint128::from_str(undel.amount.clone().expect("shoot").amount.as_str())?;
        let amnt = Decimal::from_atomics(uint_amnt, 6)?;
        println!(
            "{} →  {}ubtsg → {}",
            undel.delegator_address, amnt, undel.validator_address,
        );
        total_undel += uint_amnt;
    }
    println!(
        "Total to start unbond: {}BTSG",
        Decimal::from_atomics(total_undel, 6)?
    );

    // Uncomment and modify the export creation and serialization at the end of the function
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
        undelegates: undelegate_msgs
            .iter()
            .map(|msg| {
                let amount = msg.amount.as_ref().unwrap();
                UndelegateMsg {
                    delegator_address: msg.delegator_address.clone(),
                    validator_address: msg.validator_address.clone(),
                    amount: amount.amount.clone(),
                    denom: amount.denom.clone(),
                }
            })
            .collect(),
    };

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&export).expect("Failed to serialize messages to JSON");

    // Print the JSON to console
    println!("Delegation Messages JSON:\n{}", json);
    serialize_and_print(json.clone(), "delegation_messages.json".into());

    Ok(())
}

fn serialize_and_print(json: String, filepath: String) {
    let mut file = File::create(filepath).expect("Failed to create JSON file");
    file.write_all(json.as_bytes())
        .expect("Failed to write JSON to file");
}

// Loads array of validators getting new delegations from file, returning the total new delegations
fn load_new_delegations(fp: &str) -> AllAlignedDelegations {
    let csv = File::open(fp).expect("Could not open file");
    let mut reader = Reader::from_reader(csv);
    let mut delegations = vec![];
    let mut total = 0;

    for result in reader.records() {
        match result {
            Ok(record) => {
                let addr: String = record[0].parse().expect("Could not parse first value");
                let value2: u64 = record[1].parse().expect("Could not parse second value");
                delegations.push(Delegation {
                    del_addr: String::default(), // blank default
                    operator_addr: addr,
                    amount: Uint128::from(value2),
                });
                total += value2;
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
                continue;
            }
        }
    }
    AllAlignedDelegations {
        delegations,
        total: total.into(),
    }
}

fn optimize_delegations(
    current_delegations: Vec<Delegation>,
    target_delegations: &[Delegation],
    denom: &str,
) -> (
    Vec<MsgBeginRedelegate>,
    Vec<MsgDelegate>,
    Vec<MsgUndelegate>,
) {
    // Create maps for easier lookup
    let mut current_map: HashMap<String, Vec<Delegation>> = HashMap::new();
    let mut target_map: HashMap<String, Uint128> = HashMap::new();

    // Populate current delegations map
    for del in current_delegations {
        current_map
            .entry(del.operator_addr.clone())
            .or_default()
            .push(del.clone());
    }

    // Populate target delegation amounts
    for del in target_delegations {
        *target_map.entry(del.operator_addr.clone()).or_default() += del.amount;
    }

    let mut redelegation_msgs = Vec::<MsgBeginRedelegate>::new();
    let mut delegation_msgs = Vec::<MsgDelegate>::new();
    let mut unmatched_redelegations = Vec::<MsgUndelegate>::new();

    // First, handle validators that need reduction
    for (validator, current_dels) in &mut current_map {
        let target_amount = *target_map.get(validator).unwrap_or(&Uint128::zero());
        let total_current_del: Uint128 = current_dels.iter().map(|d| d.amount).sum();

        if total_current_del > target_amount {
            // Need to reduce delegations
            let mut reduction_needed = total_current_del - target_amount;

            // Sort current delegations by amount (largest first)
            current_dels.sort_by(|a, b| b.amount.cmp(&a.amount));

            for del in current_dels.iter_mut() {
                if reduction_needed.is_zero() {
                    break;
                }

                let redelegate_amount = del.amount.min(reduction_needed);

                // Find a target validator to redelegate to
                if let Some(dst_validator) = target_delegations
                    .iter()
                    .find(|t| t.operator_addr != *validator)
                {
                    redelegation_msgs.push(MsgBeginRedelegate {
                        delegator_address: del.del_addr.clone(),
                        validator_src_address: validator.clone(),
                        validator_dst_address: dst_validator.operator_addr.clone(),
                        amount: Some(ProtoCoin {
                            denom: denom.to_string(),
                            amount: redelegate_amount.to_string(),
                        }),
                    });

                    reduction_needed -= redelegate_amount;
                    del.amount -= redelegate_amount;
                }
            }
        }
    }

    // Then, handle validators that need additional delegation
    for target_del in target_delegations {
        let current_amount = current_map
            .get(&target_del.operator_addr)
            .map(|dels| dels.iter().map(|d| d.amount).sum())
            .unwrap_or(Uint128::zero());

        if current_amount < target_del.amount {
            let additional_needed = target_del.amount - current_amount;

            // Try to use existing redelegation sources
            let mut source_found = false;
            for (src_validator, current_dels) in &mut current_map {
                if src_validator == &target_del.operator_addr {
                    continue; // Skip same validator
                }

                for del in current_dels.iter_mut() {
                    let redelegate_amount = del.amount.min(additional_needed);

                    if !redelegate_amount.is_zero() {
                        redelegation_msgs.push(MsgBeginRedelegate {
                            delegator_address: del.del_addr.clone(),
                            validator_src_address: src_validator.clone(),
                            validator_dst_address: target_del.operator_addr.clone(),
                            amount: Some(ProtoCoin {
                                denom: denom.to_string(),
                                amount: redelegate_amount.to_string(),
                            }),
                        });

                        del.amount -= redelegate_amount;
                        source_found = true;
                        break;
                    }
                }

                if source_found {
                    break;
                }
            }

            // If no redelegation source found, add as direct delegation
            if !source_found {
                delegation_msgs.push(MsgDelegate {
                    delegator_address: target_del.del_addr.clone(), // Use a default or first DAO address
                    validator_address: target_del.operator_addr.clone(),
                    amount: Some(ProtoCoin {
                        denom: denom.to_string(),
                        amount: additional_needed.to_string(),
                    }),
                });
            }
        }
    }

    // Collect any remaining undelegated amounts
    for (validator, current_dels) in current_map {
        for del in current_dels {
            if !del.amount.is_zero() {
                unmatched_redelegations.push(MsgUndelegate {
                    delegator_address: del.del_addr,
                    validator_address: validator.clone(),
                    amount: Some(ProtoCoin {
                        denom: "ubtsg".into(),
                        amount: del.amount.to_string(),
                    }),
                });
            }
        }
    }

    (redelegation_msgs, delegation_msgs, unmatched_redelegations)
}
