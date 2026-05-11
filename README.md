# soroban-rwa-platform

[![Stellar](https://img.shields.io/badge/Stellar-Soroban-blue?logo=stellar)](https://stellar.org)
[![Soroban SDK](https://img.shields.io/badge/soroban--sdk-21.0.0-blueviolet)](https://docs.rs/soroban-sdk)
[![Wave Program](https://img.shields.io/badge/Drips-Stellar%20Wave-orange)](https://www.drips.network/wave/stellar)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![CI](https://github.com/YOUR_USERNAME/soroban-rwa-platform/actions/workflows/ci.yml/badge.svg)](https://github.com/YOUR_USERNAME/soroban-rwa-platform/actions/workflows/ci.yml)

A production-ready **Real-World Asset (RWA) tokenization and trading protocol** built on [Stellar Soroban](https://soroban.stellar.org). Register, verify, fractionalize, and trade real-world assets on-chain using USDC or XLM.

---

## Contracts

| Contract | Path | Status | Description |
|---|---|---|---|
| Asset Registry | `contracts/asset_registry` | ✅ Production | RWA registration, verification, and lifecycle management |
| Fractional | `contracts/fractional` | ✅ Production | Fractional ownership, share trading, and dividend distribution |
| Marketplace | `contracts/marketplace` | ✅ Production | Fixed-price and auction secondary market for RWA shares |

---

## Quick Start

### Prerequisites

```bash
# Install Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Add Stellar-compatible WASM target
rustup target add wasm32v1-none

# Install Stellar CLI
cargo install --locked stellar-cli
```

### Clone & Build

```bash
git clone https://github.com/YOUR_USERNAME/soroban-rwa-platform.git
cd soroban-rwa-platform

# Build native (for tests)
cargo build

# Build WASM (for deployment)
cargo build --target wasm32v1-none --release

# Run tests
cargo test
```

### Deploy to Testnet

```bash
# Configure testnet identity
stellar keys generate --global alice --network testnet

# Deploy asset registry
stellar contract deploy \
  --wasm target/wasm32-unknown-unknown/release/asset_registry.wasm \
  --source alice \
  --network testnet

# Initialize
stellar contract invoke \
  --id <CONTRACT_ID> \
  --source alice \
  --network testnet \
  -- initialize \
  --admin $(stellar keys address alice)
```

---

## Project Structure

```
soroban-rwa-platform/
├── Cargo.toml                          # Workspace root
├── contracts/
│   ├── asset_registry/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs                  # RWA registration & verification
│   ├── fractional/
│   │   ├── Cargo.toml
│   │   └── src/lib.rs                  # Fractional ownership & dividends
│   └── marketplace/
│       ├── Cargo.toml
│       └── src/lib.rs                  # Fixed-price & auction market
├── docs/
│   └── WAVE_ISSUES.md                  # Open contribution issues
├── .github/
│   └── workflows/
│       └── ci.yml                      # CI: build + test + lint
├── CONTRIBUTING.md
└── LICENSE
```

---

## Use Cases

### Real Estate
Tokenize property deeds as RWA assets. Fractional ownership lets multiple investors hold shares of a single property. Rental income is distributed as dividends proportional to share holdings.

### Gold & Commodities
Register gold bars or commodity lots with legal document hashes and appraised values. Verifiers confirm physical custody. Shares trade on the secondary marketplace.

### Secondary Market
Holders list shares for fixed-price sale or auction. Auctions auto-refund outbid participants and settle after the deadline. Platform fees are configurable at initialization.

### Supported Tokens
- **USDC** — Stellar testnet/mainnet USDC for stable-value transactions
- **XLM** — Native Stellar token via wrapped token interface

---

## Contributing

We welcome contributions! This project participates in the **[Stellar Wave Program](https://www.drips.network/wave/stellar)** — open issues carry point bounties redeemable through Drips.

See [CONTRIBUTING.md](CONTRIBUTING.md) for setup instructions, the bounty table, and PR guidelines.

See [docs/WAVE_ISSUES.md](docs/WAVE_ISSUES.md) for the full list of open issues ready to claim.

---

## License

[MIT](LICENSE) © 2026
