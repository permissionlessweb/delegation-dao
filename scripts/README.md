# BitSong Delegation Realignment Tool

DATA SOURCE: https://docs.google.com/spreadsheets/d/1Y8VGkErXrFGbmDCUDKKomn1bfMQQLIXKNJK49SYvM7M/edit?gid=0#gid=0

## TODO: 
- omit validators with existing agreements of bitsong ?
- create testing simulating delegation update


This tool automates the process of realigning delegations for BitSong DAO addresses according to a predefined distribution strategy.

## Overview

The BitSong Delegation Realignment Tool calculates and reports the necessary delegation changes to align current validator stakes with a target distribution. It:

1. Fetches current delegations for specified DAO addresses
2. Reads target delegation distributions from a CSV file
3. Calculates required adjustments to match the target distribution
4. Identifies delegations that should be removed (unbonded, unbonding, or jailed)
5. Generates a comprehensive report of changes needed

## Usage

```bash
cargo run -- --network <network>
```

Where `<network>` is one of:
- `main` - BitSong mainnet
- `testnet` - BitSong testnet
- `local` - Local development network

## Configuration

### DAO Addresses

The tool processes delegations for the following DAO addresses:
- bitsong166d42nyufxrh3jps5wx3egdkmvvg7jl6k33yut
- bitsong1nphhydjshzjevd03afzlce0xnlrnsm27hy9hgd
- bitsong1tgzday8yewn8n5j0prgsc9t5r3gg2cwnyf9jlv

### Delegation Distribution File

Target delegations are read from a CSV file located at `./data/new-delegations.csv`. The file should have two columns:
1. Validator address
2. Amount to delegate (in BTSG, will be automatically converted to ubtsg)

Example:
```csv
bitsongvaloper1abcdef...,10000
bitsongvaloper1ghijkl...,5000
```

## Delegation Logic

The tool applies the following rules:

1. **Existing validators in target list**:
   - If current delegation > target: Reduce delegation
   - If current delegation < target: Increase delegation
   - If equal: No change

2. **Validators to remove**:
   - Unbonded validators
   - Unbonding validators
   - Jailed validators
   - Any validator not in the target list

3. **New validators**:
   - Delegates specified amount to any validator in the target list that doesn't already have a delegation

## Requirements

- Rust toolchain
- Access to BitSong network (mainnet, testnet, or local)
- Properly formatted CSV file with target delegations

 