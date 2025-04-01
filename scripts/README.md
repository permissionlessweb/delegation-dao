# BitSong Delegation Realignment Tool

## Overview

The BitSong Delegation Realignment Tool is a sophisticated Rust application designed to automate and optimize the delegation strategy for BitSong DAO addresses. It provides a comprehensive solution for managing and redistributing validator stakes according to a predefined distribution strategy.

## Key Functions

### `main()`
- Parses command-line arguments for network selection
- Initializes the blockchain connection
- Executes the delegation realignment process

### `realign_delegations()`
The core function that orchestrates the entire delegation realignment process:
1. Fetches current delegations for specified DAO addresses
2. Retrieves validator status (unbonded, unbonding, jailed)
3. Loads target delegation distribution from CSV
4. Processes and matches current delegations with target distribution
5. Generates redelegation, delegation, and undelegation messages

### `optimize_delegations()`
Advanced delegation optimization function that:
- Matches current delegations with target distributions
- Handles delegation reductions and increases
- Prioritizes redelegation over new delegations
- Manages validators with insufficient or excess delegations

### `load_new_delegations()`
- Reads target delegation distribution from CSV file
- Parses validator addresses and delegation amounts
- Calculates total delegation amount

## Delegation Strategy

The tool implements a sophisticated delegation strategy:

1. **Validator Status Handling**
   - Automatically removes delegations from:
     * Unbonded validators
     * Unbonding validators
     * Jailed validators

2. **Delegation Optimization**
   - Prioritizes redelegating existing funds
   - Minimizes direct new delegations
   - Attempts to match target distribution precisely

3. **Comprehensive Reporting**
   - Generates detailed logs of:
     * Delegations to add
     * Delegations to remove
     * Undelegation requirements
   - Produces a JSON export of all proposed changes

## Usage

```bash
cargo run -- --network main