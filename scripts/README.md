# Testnet deployment script

`deploy_testnet.sh` builds the workspace contracts, deploys them to Stellar
Testnet, initializes the protocol, and runs a small end-to-end RWA flow:

1. Register a sample real estate asset.
2. Verify it with the deployer account.
3. Create a fractional offering with 1,000 shares at 10 USDC each.
4. Create a fixed-price marketplace listing for 100 shares.

The script is intended for contributor smoke testing and Wave issue validation.

## Prerequisites

- Rust with the `wasm32v1-none` target:

  ```bash
  rustup target add wasm32v1-none
  ```

- Stellar CLI installed and available as `stellar`:

  ```bash
  cargo install --locked stellar-cli
  ```

## Usage

From the repository root:

```bash
chmod +x scripts/deploy_testnet.sh
./scripts/deploy_testnet.sh
```

By default, the script:

- uses the `testnet` network
- creates or reuses a funded identity named `rwa-testnet-deployer`
- resolves the Stellar Asset Contract for the Testnet USDC asset documented by Stellar:
  `USDC:GCYEIQEWOCTTSA72VPZ6LYIZIK4W4KNGJR72UADIXUXG45VDFRVCQTYE`
- writes Stellar CLI aliases for the token and each deployed contract

The script deploys fresh contract instances on each run, so re-running it with
the same identity will not fail because previous contracts were already
initialized.

## Configuration

All configuration is provided through environment variables:

| Variable | Default | Purpose |
| --- | --- | --- |
| `NETWORK` | `testnet` | Stellar CLI network name |
| `STELLAR_IDENTITY` | `rwa-testnet-deployer` | Local Stellar CLI identity |
| `USDC_ASSET` | `USDC:GCYEIQEWOCTTSA72VPZ6LYIZIK4W4KNGJR72UADIXUXG45VDFRVCQTYE` | Stellar asset wrapped as the token contract |
| `TOKEN_CONTRACT_ID` | empty | Use an existing token contract instead of deploying the asset contract |
| `WASM_TARGET` | `wasm32v1-none` | Rust target used for Stellar-compatible WASM |
| `ASSET_REGISTRY_ALIAS` | `rwa_asset_registry` | Alias for the asset registry contract |
| `FRACTIONAL_ALIAS` | `rwa_fractional` | Alias for the fractional contract |
| `MARKETPLACE_ALIAS` | `rwa_marketplace` | Alias for the marketplace contract |
| `TOKEN_ALIAS` | `rwa_usdc` | Alias for the token contract |
| `PRICE_PER_SHARE` | `100000000` | 10 USDC with 7 decimal places |
| `TOTAL_SHARES` | `1000` | Sample offering share count |
| `MIN_PURCHASE` | `1` | Minimum purchase quantity for the sample offering |
| `LISTING_SHARE_COUNT` | `100` | Shares listed in the marketplace sample |
| `FEE_RATE_BPS` | `100` | 1% fee rate for fractional and marketplace contracts |
| `LISTING_DEADLINE_LEDGER` | `429496729` | Future ledger deadline for the sample listing |

Example using a pre-existing token contract:

```bash
TOKEN_CONTRACT_ID=CDLZFC3SYJYDZT7K67VZ75HPJVIEUVZPHCE7AKQHOUWH2DJ4XKCRJ5QG \
  ./scripts/deploy_testnet.sh
```

## Output

At the end of a successful run the script prints:

- network
- identity
- deployer account
- token contract ID
- asset registry contract ID
- fractional contract ID
- marketplace contract ID
- sample asset, offering, and listing IDs

The individual `stellar contract deploy` and `stellar contract invoke` commands
also print their normal transaction output while the script runs.
