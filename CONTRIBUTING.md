# Contributing to soroban-rwa-platform

Thank you for your interest in contributing! This project is part of the **[Stellar Wave Program](https://www.drips.network/wave/stellar)** on Drips. Merged contributions earn points redeemable as funding from the Wave pool.

---

## Dev Setup

```bash
# 1. Install Rust stable
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup default stable

# 2. Add WASM target
rustup target add wasm32-unknown-unknown

# 3. Install Stellar CLI
cargo install --locked stellar-cli --features opt

# 4. Clone and build
git clone https://github.com/YOUR_USERNAME/soroban-rwa-platform.git
cd soroban-rwa-platform
cargo build
```

---

## Wave Bounty Table

Issues are labeled with complexity and point values. Points are distributed via the Drips Stellar Wave pool.

| Complexity | Points | Description |
|---|---|---|
| `complexity: trivial` | 100 pts | Doc comments, minor fixes, small refactors |
| `complexity: medium` | 150 pts | New features, test suites, scripts |
| `complexity: high` | 200 pts | Protocol extensions, security features, major additions |

Browse open issues in [docs/WAVE_ISSUES.md](docs/WAVE_ISSUES.md) or the [GitHub Issues tab](../../issues).

---

## PR Guidelines

1. **Fork** the repo and create a branch: `git checkout -b feat/your-feature`
2. **Write tests** for any new logic. All contracts must maintain passing tests.
3. **Run lint and tests** before opening a PR (see commands below).
4. **Reference the issue** in your PR description: `Closes #N`
5. **Keep PRs focused** — one feature or fix per PR.
6. PRs require at least one maintainer approval before merge.

---

## Test & Lint Commands

```bash
# Run all tests (native)
cargo test

# Check formatting
cargo fmt --all -- --check

# Run Clippy (zero warnings policy)
cargo clippy --all-targets --all-features -- -D warnings

# Build WASM release
cargo build --target wasm32-unknown-unknown --release
```

---

## Code Style

- All contracts use `#![no_std]` — no standard library.
- Use `env.storage().persistent()` for per-entity state, `env.storage().instance()` for global config.
- Emit events via `env.events().publish()` on every state change.
- Prefer `assert!` with descriptive messages over `panic!` for user-facing errors.
- Run `cargo fmt` before committing.

---

## Questions?

Open a [GitHub Discussion](../../discussions) or reach out via the Stellar Discord `#soroban-dev` channel.
