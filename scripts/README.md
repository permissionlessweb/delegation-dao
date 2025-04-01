# BitSong Delegation Realignment Tool

## Overview

The BitSong Delegation Realignment Tool is a sophisticated Rust application designed to automate and optimize the delegation strategy for BitSong DAO addresses. It provides a comprehensive solution for managing and redistributing validator stakes according to a predefined distribution strategy.

## DATA SOURCE
Google docs: https://docs.google.com/spreadsheets/d/1Y8VGkErXrFGbmDCUDKKomn1bfMQQLIXKNJK49SYvM7M/edit?gid=1983149619#gid=1983149619 

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

## Test Suite

The tool includes several tests to ensure accuracy and reliability:

### `test_load_obligated_delegations_file()`
- Verifies the CSV file loading functionality
- Ensures proper parsing of validator addresses and delegation amounts
- Confirms the total delegation amount matches the expected value
- Checks that the number of validators matches the predefined constant

### `test_yes_no_load_obligated_delegations_file()`
- Tests CSV loading with and without headers
- Determines the correct CSV format by comparing totals
- Provides diagnostics about CSV structure

### `test_accuracy_delegations_message_json()`
- Placeholder for validating the JSON export accuracy

### Runtime Verification
In addition to unit tests, the tool includes runtime verification:

1. **Final State Verification**
   - Simulates the application of all delegations, redelegations, and undelegations
   - Compares the simulated final state with the target obligations
   - Reports discrepancies between final and target states
   - Identifies any validators with unexpected delegations

2. **Max Entries Constraint Verification**
   - Checks for potential violations of Cosmos SDK constraints
   - Ensures no more than 7 delegations exist per (delegator, validator) pair
   - Ensures no more than 7 redelegations per (delegator, source, destination) triplet
   - Fails with clear error messages if constraints would be violated

## Usage

```bash
cargo run -- --network main
```