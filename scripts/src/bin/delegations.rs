use std::{collections::HashMap, fs::File, io::Write, mem::zeroed, str::FromStr};

use clap::Parser;
use cosmos_sdk_proto::cosmos::{
    base::{query::v1beta1::PageRequest, v1beta1::Coin as ProtoCoin},
    staking::v1beta1::{
        DelegationResponse, MsgBeginRedelegate, MsgDelegate, MsgDelegateResponse, MsgUndelegate,
    },
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Decimal, Uint128};
use csv::{Reader, ReaderBuilder};
use cw_orch::{
    daemon::{
        queriers::{Bank, Staking},
        DaemonBuilder, TxSender,
    },
    environment::{ChainKind, NetworkInfo},
    prelude::*,
};
use serde::Serialize;
use tokio::runtime::Runtime;

pub const TOTAL_OBLIGATED_VALIDATORS: usize = 33;
pub const TOTAL_OBLIGATED_DELEGATED_BTSG: Uint128 = Uint128::new(9_999_980_000_000u128);
pub const NEW_DELS_FILE: &str = "./src/bin/data/new-delegations.csv";
pub const RAW_MSG_JSON: &str = "delegation_messages.json";

#[cw_serde]
struct DelegationDaoEntity {
    dao_add: String,
    current_balance: Coin,
    current_delegation: Uint128,
    obligated_delegation: Uint128,
    total_delegation_count: usize,
}

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

#[cw_serde]
struct AllAlignedDelegations {
    delegations: Vec<Delegation>,
    total: Uint128,
}

#[cw_serde]
struct RedelegateMsg {
    delegator_address: String,
    validator_src_address: String,
    validator_dst_address: String,
    amount: String,
    denom: String,
}

#[cw_serde]
struct UndelegateMsg {
    delegator_address: String,
    validator_address: String,
    amount: String,
    denom: String,
}

#[cw_serde]
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

#[cw_serde]
struct Redelegations {
    data: Vec<RedelegateMsg>,
    count: usize,
    total_ubtsg: Uint128,
}

#[cw_serde]
struct Delegations {
    data: Vec<DelegateMsg>,
    count: usize,
    total_ubtsg: Uint128,
}

#[cw_serde]
struct Undelegations {
    data: Vec<UndelegateMsg>,
    count: usize,
    total_ubtsg: Uint128,
}

#[cw_serde]
struct MessageExport {
    redelegations: Redelegations,
    delegations: Delegations,
    undelegates: Undelegations,
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
    let bank_query_client: Bank = chain.querier();

    // Create a new runtime for async execution
    let rt = Runtime::new()?;

    // Execute the async function using the runtime
    if let Err(err) = rt.block_on(realign_delegations(
        staking_query_client,
        bank_query_client,
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
    bank_client: Bank,
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
    let mut all_oblgated_dels = load_new_delegations(NEW_DELS_FILE, false);
    let mut obligated_delegations = all_oblgated_dels.delegations;
    let total_obligated_delegations = Uint128::from(all_oblgated_dels.total);

    println!("Running Bitsong Delegation Realignment Protocol...");
    println!(
        "{} dels with {}ubtsg",
        obligated_delegations.len(),
        Decimal::from_atomics(
            total_obligated_delegations.checked_mul(1000000u128.into())?,
            6
        )?
        .to_string()
    );

    // collect all dao delegations
    let mut all_dao_delegations = Vec::new();
    let mut dao_entities = Vec::new();
    let ommited_vals: Vec<String> = vec![
        "bitsongvaloper19ah9302mh80pvv5zeztdr6qcqk6z52frn6rjj5".into(),
        "bitsongvaloper1wf3q0a3uzechxvf27reuqts8nqm45sn2yq26g3".into(),
        "bitsongvaloper10fg3yklae97g8ueh5ut29mlwz8fdr6z8zrak6x".into(),
        "bitsongvaloper1fkj2cn209yeexxyets98evrcmmds23hck0lyzq".into(),
        "bitsongvaloper1wetqg989uyj3mpk07h8yt3qvu2cdlsv7fp3zda".into(),
        "bitsongvaloper1jxv0u20scum4trha72c7ltfgfqef6nscl86wxa".into(),
    ];
    for dao in &dao_addrs {
        let mut next_key = None;
        loop {
            let response = staking_query_client
                ._delegator_delegations(&Addr::unchecked(dao), next_key)
                .await?;

            all_dao_delegations.extend(response.delegation_responses);
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

        let entity_btsg_addr = bank_client
            ._balance(&Addr::unchecked(dao), Some("ubtsg".into()))
            .await?;

        let total_non_team_de: Vec<&DelegationResponse> = all_dao_delegations
            .iter()
            .filter(|a| {
                !ommited_vals.contains(&a.delegation.clone().expect("msg").validator_address)
            })
            .collect();

        let new_total_delegation_amount = obligated_delegations
            .clone()
            .iter()
            .find(|d| d.del_addr == *dao)
            .map(|a| a.amount)
            .into_iter()
            .sum();
        dao_entities.push(DelegationDaoEntity {
            dao_add: dao.to_string(),

            current_balance: entity_btsg_addr[0].clone(),
            total_delegation_count: total_non_team_de.len(),
            current_delegation: total_non_team_de
                .clone()
                .iter()
                .map(|a| Uint128::from_str(&a.balance.clone().expect("msg").amount).expect("msg"))
                .sum(),
            obligated_delegation: new_total_delegation_amount,
        });
    }

    // current_vals - array of validators and the DAOs delegations to them
    let mut current_vals: Vec<AlignedValidator> = Vec::new();

    // all delegations, enumerated
    for (i, dels) in all_dao_delegations.clone().iter_mut().enumerate() {
        let del = dels.delegation.clone().unwrap();
        if !ommited_vals.contains(&del.validator_address) {
            let balance = Uint128::from_str(&dels.balance.clone().unwrap().amount)
                .expect("Failed to parse balance");
            if !balance.is_zero() {
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
                    // initialize aligned validator
                    let target_amount = obligated_delegations
                        .iter()
                        .find(|a| a.operator_addr == del.validator_address)
                        .map_or(Uint128::zero(), |a| a.amount);
                    if target_amount != Uint128::zero() {
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
        } else {
            // do not add to this list of delegators, and disregard remaining list
            all_dao_delegations.remove(i);
        }
    }
    //assert we omit private agreement validator operators
    assert!(current_vals
        .iter()
        .all(|cv| !ommited_vals.contains(&cv.operator_addr)));

    // Add any completely new validators from aligned_vals that don't exist in current_vals
    for obligated in &obligated_delegations {
        if !current_vals
            .iter()
            .any(|cv| cv.operator_addr == obligated.operator_addr)
        {
            current_vals.push(AlignedValidator {
                operator_addr: obligated.operator_addr.clone(),
                current_delegations: Vec::new(),
                new_delegation_amount: obligated.amount,
            });
        }
    }

    // Modify the main delegation processing to use this debug function
    debug_delegation_tracking(&all_dao_delegations, &obligated_delegations)?;

    let mut all_redels: Vec<Delegation> = Vec::new();
    let mut all_dels: Vec<Delegation> = Vec::new();
    let mut del_total = Uint128::zero();
    let mut redel_total = Uint128::zero();
    let mut undel_total = Uint128::zero();

    for mut val in current_vals {
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

        let sum_dao_delegation = val
            .current_delegations
            .iter()
            .map(|a| a.amount)
            .sum::<Uint128>();

        if val.new_delegation_amount.is_zero() || is_unbonded || is_unbonding || is_jailed {
            let reason = if is_unbonded {
                "unbonded"
            } else if is_unbonding {
                "unbonding"
            } else if val.new_delegation_amount.is_zero() {
                "no-obligation-this-round"
            } else {
                "jailed"
            };

            println!("val.current_delegations: {:#?}", val.current_delegations);
            println!(
                "Will remove {}ubtsg from {} validator {}",
                sum_dao_delegation, reason, val.operator_addr
            );

            redel_total += sum_dao_delegation;
            all_redels.extend(val.current_delegations);
            continue;
        } else if total_current_del > val.new_delegation_amount {
            // Need to reduce delegations, get difference and try to create
            let diff = total_current_del
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
            all_redels.extend(redelegations_to_add.clone());
            println!(
                "Will remove {}ubtsg from {}",
                redelegations_to_add
                    .iter()
                    .map(|a| a.amount)
                    .sum::<Uint128>(),
                val.operator_addr
            );

            // only set remaining difference to unbond.
        } else if total_current_del < val.new_delegation_amount {
            //  need to delegate
            let diff = val
                .new_delegation_amount
                .checked_sub(total_current_del)
                .expect("darn");

            let mut old_delegations = val.current_delegations.clone();
            old_delegations.sort_by(|a, b| b.amount.cmp(&a.amount));

            let mut remaining_diff = diff;
            let mut delegation_to_add = Vec::new();

            for old in old_delegations {
                if remaining_diff.is_zero() {
                    break; // We've consumed all the difference
                }

                if old.amount <= remaining_diff {
                    // We can use this entire delegation
                    remaining_diff = remaining_diff
                        .checked_sub(old.amount)
                        .expect("subtraction overflow");
                    del_total += old.amount;
                    delegation_to_add.push(old);
                } else {
                    // We need only part of this delegation
                    let mut partial_delegation = old.clone();
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
        all_dao_delegations
            .iter()
            .map(|a| Delegation {
                del_addr: a.delegation.clone().expect("msg").delegator_address,
                operator_addr: a.delegation.clone().expect("msg").validator_address,
                amount: Uint128::from_str(a.balance.clone().expect("msg").amount.as_str()).unwrap(),
            })
            .collect(), // Current delegations
        &obligated_delegations, // Target delegations
        "ubtsg",
    );

    // Print summary of delegation changes
    println!("\n--- DELEGATIONS TO ADD ---");
    let mut total_del = Uint128::zero();
    for del in &delegation_msgs {
        let uint_amnt = Uint128::from_str(del.amount.clone().expect("shoot").amount.as_str())?;
        total_del += uint_amnt;
        // let amnt = Decimal::from_atomics(uint_amnt, 6)?;
        // println!("  {} → {} BTSG", del.validator_address, amnt);
    }

    println!(
        "Total to delegate: {}. Amount: {}",
        Decimal::from_atomics(total_del, 6)?,
        delegation_msgs.len()
    );
    // assert_eq!(
    //     Decimal::from_atomics(del_total, 6)?,
    //     Decimal::from_atomics(total_del, 6)?
    // );

    println!("\n--- DELEGATIONS TO REMOVE ---");
    let mut total_redel = Uint128::zero();
    // println!("Total to redelegate: {}", redel_map.1);
    for redel in &redelegation_msgs {
        let uint_amnt = Uint128::from_str(redel.amount.clone().expect("shoot").amount.as_str())?;
        let amnt = Decimal::from_atomics(uint_amnt, 6)?;
        total_redel += uint_amnt;
    }
    println!(
        "Total to redelegate: {} BTSG. amount: {}",
        Decimal::from_atomics(total_redel, 6)?,
        redelegation_msgs.len()
    );

    let mut total_undel = Uint128::zero();
    for undel in &undelegate_msgs {
        let uint_amnt = Uint128::from_str(undel.amount.clone().expect("shoot").amount.as_str())?;
        let amnt = Decimal::from_atomics(uint_amnt, 6)?;
        println!(
            "{} →  {}BTSG → {}",
            undel.delegator_address, amnt, undel.validator_address,
        );
        total_undel += uint_amnt;
    }

    println!(
        "Total to start unbond: {} BTSG",
        Decimal::from_atomics(total_undel, 6)?
    );

    // Uncomment and modify the export creation and serialization at the end of the function
    let export = MessageExport {
        redelegations: Redelegations {
            data: redelegation_msgs
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
            count: redelegation_msgs.len(),
            total_ubtsg: redel_total,
        },
        delegations: Delegations {
            data: delegation_msgs
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
            count: delegation_msgs.len(),
            total_ubtsg: del_total,
        },
        undelegates: Undelegations {
            data: undelegate_msgs
                .iter()
                .map(|msg| UndelegateMsg {
                    delegator_address: msg.delegator_address.clone(),
                    validator_address: msg.validator_address.clone(),
                    amount: msg.amount.clone().expect("msg").amount,
                    denom: msg.amount.clone().expect("msg").denom,
                })
                .collect(),
            count: undelegate_msgs.len(),
            total_ubtsg: undel_total,
        },
    };

    // Serialize to JSON
    let json = serde_json::to_string_pretty(&export).expect("Failed to serialize messages to JSON");

    serialize_and_print(json.clone(), RAW_MSG_JSON.to_string());

    // assert with the new information that the obligated validators will have the correct balance once delegations are applied
    verify_final_state(RAW_MSG_JSON, &all_dao_delegations, &obligated_delegations)?;

    Ok(())
}

fn serialize_and_print(json: String, filepath: String) {
    let mut file = File::create(filepath).expect("Failed to create JSON file");
    file.write_all(json.as_bytes())
        .expect("Failed to write JSON to file");
}

// Loads array of validators getting new delegations from file, returning the total new delegations
fn load_new_delegations(fp: &str, has_header: bool) -> AllAlignedDelegations {
    let file = File::open(fp).expect("Could not open file");

    // Create a reader with configurable header setting
    let mut rdr = ReaderBuilder::new()
        .has_headers(has_header)
        .from_reader(file);

    let mut delegations = vec![];
    let mut total = Uint128::zero();

    for result in rdr.records() {
        match result {
            Ok(record) => {
                // Ensure there are at least two fields
                if record.len() < 2 {
                    eprintln!(
                        "Invalid record format: expected at least 2 fields, got {}",
                        record.len()
                    );
                    continue;
                }

                let addr = match record[0].parse::<String>() {
                    Ok(addr) => addr,
                    Err(e) => {
                        eprintln!("Error parsing address: {}", e);
                        continue;
                    }
                };

                let amount = match record[1].parse::<Uint128>() {
                    Ok(num) => num,
                    Err(e) => {
                        eprintln!("Error parsing amount: {}", e);
                        continue;
                    }
                };

                delegations.push(Delegation {
                    del_addr: String::default(),
                    operator_addr: addr,
                    amount,
                });

                total += amount;
            }
            Err(e) => {
                eprintln!("Error reading record: {}", e);
            }
        }
    }

    println!(
        "Loaded {} delegations with total amount {}",
        delegations.len(),
        total
    );
    AllAlignedDelegations { delegations, total }
}

fn optimize_delegations(
    current_delegations: Vec<Delegation>,
    obligated_delegations: &[Delegation],
    denom: &str,
) -> (
    Vec<MsgBeginRedelegate>,
    Vec<MsgDelegate>,
    Vec<MsgUndelegate>,
) {
    // Maps of all current and obligated delegations
    let mut old_delegations: HashMap<String, Vec<Delegation>> = HashMap::new();
    let mut obligated_delegations_map: HashMap<String, Uint128> = HashMap::new();

    // Preprocessing: Assert current and obligated delegations total
    let mut total_current_delegation = Uint128::zero();
    let mut total_obligated_delegation = Uint128::zero();

    let mut redelegation_msgs = Vec::<MsgBeginRedelegate>::new();
    let mut delegation_msgs = Vec::<MsgDelegate>::new();
    let mut undelegate_msgs = Vec::<MsgUndelegate>::new();

    // save old delegations hash map with validator as key
    for del in current_delegations {
        total_current_delegation += del.amount;
        old_delegations
            .entry(del.operator_addr.clone())
            .or_default()
            .push(del.clone());
    }



    // save desired delegations hash map with validator as key
    for del in obligated_delegations {
        let amount = del.amount;
        total_obligated_delegation += amount;
        *obligated_delegations_map
            .entry(del.operator_addr.clone())
            .or_default() += amount;
    }
    assert_eq!(total_obligated_delegation, TOTAL_OBLIGATED_DELEGATED_BTSG);
    // First pass: Process validators that need additional delegations
    for target_del in obligated_delegations {
        let target_validator = &target_del.operator_addr;
        let target_amount = target_del.amount;

        let current_amount = old_delegations
            .get(target_validator)
            .map(|dels| dels.iter().map(|d| d.amount).sum())
            .unwrap_or(Uint128::zero());

        let mut additional = Uint128::zero();
        if current_amount < target_amount {
            additional = target_amount.checked_sub(current_amount).expect("");
        }

        if additional.is_zero() {
            continue;
        }

        // Try to source from other validators
        let mut remaining_needed = additional;
        for (src_validator, current_dels) in &mut old_delegations {
            if src_validator == target_validator {
                continue; // Skip same validator
            }

            // Check if source validator has excess over its own target
            let src_target = obligated_delegations_map
                .get(src_validator)
                .copied()
                .unwrap_or(Uint128::zero());
            // current delegation sum
            let src_current: Uint128 = current_dels.iter().map(|d| d.amount).sum();

            if src_current <= src_target {
                continue; // Don't take from validators that need their delegations
            }

            // Sort current delegations to prioritize larger amounts
            current_dels.sort_by(|a, b| b.amount.cmp(&a.amount));

            for del in current_dels.iter_mut() {
                if remaining_needed.is_zero() {
                    break;
                }

                // Safely calculate redelegate amount
                let excess = src_current.saturating_sub(src_target);
                let available_to_redelegate = del.amount.min(excess);
                let redelegate_amount = available_to_redelegate.min(remaining_needed);

                if !redelegate_amount.is_zero() {
                    redelegation_msgs.push(MsgBeginRedelegate {
                        delegator_address: del.del_addr.clone(),
                        validator_src_address: src_validator.clone(),
                        validator_dst_address: target_validator.clone(),
                        amount: Some(ProtoCoin {
                            denom: denom.to_string(),
                            amount: redelegate_amount.to_string(),
                        }),
                    });

                    // Safely update amounts
                    remaining_needed = remaining_needed
                        .checked_sub(redelegate_amount)
                        .unwrap_or(Uint128::zero());

                    del.amount = del
                        .amount
                        .checked_sub(redelegate_amount)
                        .unwrap_or(Uint128::zero());
                }
            }

            // Break if no more needed
            if remaining_needed.is_zero() {
                break;
            }
        }

        // If still need delegation, add direct delegation
        if !remaining_needed.is_zero() {
            delegation_msgs.push(MsgDelegate {
                delegator_address: target_del.del_addr.clone(), // Use a default or first DAO address
                validator_address: target_validator.clone(),
                amount: Some(ProtoCoin {
                    denom: denom.to_string(),
                    amount: remaining_needed.to_string(),
                }),
            });
        }
    }

    // Second pass: Handle undelegations for validators with excess
    for (validator, dels) in &old_delegations {
        let current_total: Uint128 = dels.iter().map(|d| d.amount).sum();
        let target = obligated_delegations_map
            .get(validator)
            .copied()
            .unwrap_or(Uint128::zero());

        // Check if there's excess that needs to be undelegated
        if current_total > target {
            let excess = current_total.checked_sub(target).unwrap_or(Uint128::zero());

            if !excess.is_zero() {
                let mut remaining_excess = excess;

                // Process each delegation for this validator
                for del in dels {
                    if remaining_excess.is_zero() {
                        break;
                    }

                    let undelegate_amount = del.amount.min(remaining_excess);

                    if !undelegate_amount.is_zero() {
                        undelegate_msgs.push(MsgUndelegate {
                            delegator_address: del.del_addr.clone(),
                            validator_address: validator.clone(),
                            amount: Some(ProtoCoin {
                                denom: denom.to_string(),
                                amount: undelegate_amount.to_string(),
                            }),
                        });

                        remaining_excess = remaining_excess
                            .checked_sub(undelegate_amount)
                            .unwrap_or(Uint128::zero());
                    }
                }
            }
        }
    }

    // Comprehensive logging
    println!("Redelegation Msgs: {}", redelegation_msgs.len());
    let total_redelegation = redelegation_msgs
        .iter()
        .map(|msg| Uint128::from_str(msg.amount.clone().expect("amount").amount.as_str()).unwrap())
        .sum::<Uint128>();

    println!("Delegation Msgs: {}", delegation_msgs.len());
    let total_delegation = delegation_msgs
        .iter()
        .map(|msg| Uint128::from_str(msg.amount.clone().expect("amount").amount.as_str()).unwrap())
        .sum::<Uint128>();

    println!("Undelegate Msgs: {}", undelegate_msgs.len());
    let total_undelegation = undelegate_msgs
        .iter()
        .map(|msg| Uint128::from_str(msg.amount.clone().expect("amount").amount.as_str()).unwrap())
        .sum::<Uint128>();

    println!("Total Redelegation Amount: {}", total_redelegation);
    println!("Total Delegation Amount: {}", total_delegation);
    println!("Total Undelegation Amount: {}", total_undelegation);

    (redelegation_msgs, delegation_msgs, undelegate_msgs)
}

// Add detailed logging to track delegation totals
fn debug_delegation_tracking(
    current_dao_delegations: &[DelegationResponse],
    obligated_dao_delegations: &[Delegation],
) -> anyhow::Result<()> {
    // Track total current delegations
    let total_current_delegations: Uint128 = current_dao_delegations
        .iter()
        .map(|a| Uint128::from_str(a.balance.clone().expect("msg").amount.as_str()).unwrap())
        .sum();

    // Track total target delegations
    let total_obligated_delegations: Uint128 =
        obligated_dao_delegations.iter().map(|d| d.amount).sum();

    println!("\n--- DELEGATION TOTAL DEBUGGING ---");
    println!(
        "Determined Current Delegations : {}  
         Determine Obligated Delegations: {}",
        Decimal::from_atomics(total_current_delegations, 6)?,
        Decimal::from_atomics(total_obligated_delegations, 6)?
    );

    // Print detailed breakdown of current delegations
    println!("\nCurrent Delegation Breakdown:");
    let mut detailed_current_dels = current_dao_delegations
        .iter()
        .map(|a| {
            let balance =
                Uint128::from_str(a.balance.clone().expect("msg").amount.as_str()).unwrap();
            (
                a.delegation.clone().expect("msg").validator_address,
                balance,
            )
        })
        .collect::<Vec<_>>();

    detailed_current_dels.sort_by(|a, b| b.1.cmp(&a.1));
    let mut sum = Uint128::zero();
    for (validator, amount) in detailed_current_dels {
        sum += amount;
    }
    let dec = Decimal::from_atomics(sum, 6)?;
    println!("sum: {}", dec);

    // Print detailed breakdown of target delegations
    println!("\n Obligated Delegation Breakdown:");
    let mut detailed_obligated_delegations = obligated_dao_delegations
        .iter()
        .map(|d| (d.operator_addr.clone(), d.amount))
        .collect::<Vec<_>>();

    detailed_obligated_delegations.sort_by(|a, b| b.1.cmp(&a.1));

    sum = Uint128::zero();
    for (_, amount) in detailed_obligated_delegations {
        sum += amount;
    }
    println!("sum: {}", sum);

    Ok(())
}

fn verify_final_state(
    json_file: &str,
    current_delegations: &[DelegationResponse],
    obligated_delegations: &[Delegation],
) -> anyhow::Result<()> {
    println!("\n--- VERIFYING FINAL VALIDATOR STATE ---");

    // Read and parse the JSON file
    let file_content = std::fs::read_to_string(json_file)?;
    let export: MessageExport = serde_json::from_str(&file_content)?;

    // Create maps for current and target delegations by validator
    let mut current_by_validator: HashMap<String, Uint128> = HashMap::new();
    let mut obligated_by_validator: HashMap<String, Uint128> = HashMap::new();

    // Populate current delegations map
    for del in current_delegations {
        let validator = del
            .delegation
            .as_ref()
            .expect("msg")
            .validator_address
            .clone();
        let amount = Uint128::from_str(&del.balance.as_ref().expect("msg").amount)?;
        *current_by_validator.entry(validator).or_default() += amount;
    }

    // Populate obligated delegations map
    for del in obligated_delegations {
        *obligated_by_validator
            .entry(del.operator_addr.clone())
            .or_default() += del.amount;
    }

    // Apply all changes from the export to simulate final state
    let mut final_state = current_by_validator.clone();

    // Apply redelegations (subtract from source, add to destination)
    for redel in &export.redelegations.data {
        let amount = Uint128::from_str(&redel.amount)?;
        *final_state
            .entry(redel.validator_src_address.clone())
            .or_default() -= amount;
        *final_state
            .entry(redel.validator_dst_address.clone())
            .or_default() += amount;
    }

    // Apply delegations (add to validator)
    for del in &export.delegations.data {
        let amount = Uint128::from_str(&del.amount)?;
        *final_state
            .entry(del.validator_address.clone())
            .or_default() += amount;
    }

    // Apply undelegations (subtract from validator)
    for undel in &export.undelegates.data {
        let amount = Uint128::from_str(&undel.amount)?;
        *final_state
            .entry(undel.validator_address.clone())
            .or_default() -= amount;
    }

    // Verify the final state matches the obligated state
    let mut discrepancies = Vec::new();
    let mut total_final = Uint128::zero();
    let mut total_obligated = Uint128::zero();

    // Check each validator's final state against obligation
    for (validator, &obligated_amount) in &obligated_by_validator {
        let final_amount = final_state
            .get(validator)
            .copied()
            .unwrap_or(Uint128::zero());
        total_obligated += obligated_amount;
        total_final += final_amount;

        if final_amount != obligated_amount {
            discrepancies.push((
                validator.clone(),
                final_amount,
                obligated_amount,
                final_amount
                    .checked_sub(obligated_amount)
                    .unwrap_or_else(|_| {
                        obligated_amount
                            .checked_sub(final_amount)
                            .unwrap_or(Uint128::zero())
                    }),
            ));
        }
    }

    // Print verification results
    println!(
        "Total final delegation amount: {}",
        Decimal::from_atomics(total_final, 6)?
    );
    println!(
        "Total obligated delegation amount: {}",
        Decimal::from_atomics(total_obligated, 6)?
    );

    if discrepancies.is_empty() {
        println!(
            "✅ VERIFICATION PASSED: All validators have the correct obligated delegation amount"
        );
    } else {
        println!(
            "❌ VERIFICATION FAILED: Found {} validators with discrepancies",
            discrepancies.len()
        );

        // Sort discrepancies by difference amount (largest first)
        discrepancies.sort_by(|a, b| b.3.cmp(&a.3));

        println!("\nTop discrepancies:");
        for (validator, final_amount, obligated_amount, diff) in discrepancies.iter().take(10) {
            println!(
                "Validator {}: Final={}, Obligated={}, Diff={}",
                validator,
                Decimal::from_atomics(*final_amount, 6)?,
                Decimal::from_atomics(*obligated_amount, 6)?,
                Decimal::from_atomics(*diff, 6)?
            );
        }
    }

    // Check for validators with redelegations or undelegations that aren't in obligated_delegations
    let mut unexpected_validators = Vec::new();
    for validator in final_state.keys() {
        if !obligated_by_validator.contains_key(validator)
            && final_state
                .get(validator)
                .copied()
                .unwrap_or(Uint128::zero())
                > Uint128::zero()
        {
            unexpected_validators.push((
                validator.clone(),
                final_state
                    .get(validator)
                    .copied()
                    .unwrap_or(Uint128::zero()),
            ));
        }
    }

    if !unexpected_validators.is_empty() {
        println!("\n⚠️ WARNING: Found {} validators with delegations that are not in the obligated list:", unexpected_validators.len());
        for (validator, amount) in unexpected_validators {
            println!(
                "Validator {}: Amount={}",
                validator,
                Decimal::from_atomics(amount, 6)?
            );
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_obligated_delegations_file() -> anyhow::Result<()> {
        let aad = load_new_delegations(NEW_DELS_FILE, false);
        // Check the calculated total from the struct

        // Calculate and check the sum of individual
        let obligated_delegation_sum: Uint128 = aad.delegations.iter().map(|a| a.amount).sum();
        println!("Total from struct : {}", aad.total);
        println!("Sum of delegations: {}", obligated_delegation_sum);

        // Both should match the expected value
        assert_eq!(aad.delegations.len(), TOTAL_OBLIGATED_VALIDATORS);
        assert_eq!(aad.total.u128(), TOTAL_OBLIGATED_DELEGATED_BTSG.u128());
        assert_eq!(
            obligated_delegation_sum.u128(),
            TOTAL_OBLIGATED_DELEGATED_BTSG.u128()
        );

        Ok(())
    }

    // Add this function near the end of realign_delegations,
    // just before the final Ok(()) return

    #[test]
    fn test_accuracy_delegations_message_json() -> anyhow::Result<()> {
        Ok(())
    }

    // Usage in test
    #[test]
    fn test_yes_no_load_obligated_delegations_file() -> anyhow::Result<()> {
        // Try with both header settings to see which matches expected value
        let aad_with_header = load_new_delegations(NEW_DELS_FILE, true);
        let aad_without_header = load_new_delegations(NEW_DELS_FILE, false);

        println!(
            "With header: {} delegations, total {}",
            aad_with_header.delegations.len(),
            aad_with_header.total
        );

        println!(
            "Without header: {} delegations, total {}",
            aad_without_header.delegations.len(),
            aad_without_header.total
        );

        // Check which one matches the expected value
        let expected = 9_999_980_000_000u128;

        if aad_with_header.total.u128() == expected {
            println!("CSV has a header row");
            assert_eq!(aad_with_header.total.u128(), expected);
        } else if aad_without_header.total.u128() == expected {
            println!("CSV does not have a header row");
            assert_eq!(aad_without_header.total.u128(), expected);
        } else {
            panic!("Neither header configuration matches expected total");
        }

        Ok(())
    }
}
