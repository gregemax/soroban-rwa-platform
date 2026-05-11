# Wave Issues — Open Contribution Bounties

Copy-paste any issue below directly into GitHub Issues. Each issue is labeled, scoped, and ready to claim.

---

## Issue #1 — Full Test Suite: Asset Registry

**Title:** `Add full test suite for asset_registry contract`

**Labels:** `good first issue`, `testing`, `complexity: medium`, `wave: 150pts`

**Description:**
The `asset_registry` contract has no automated tests. We need a comprehensive test suite covering all public functions using `soroban-sdk`'s `testutils` feature.

**Tasks:**
- [ ] Test `initialize` — verify admin is set, panic on double-init
- [ ] Test `register_verifier` / `remove_verifier` — admin-only, event emitted
- [ ] Test `register_asset` — correct defaults, `Pending` status, event emitted
- [ ] Test `verify_asset` — verifier-only, status transitions, `verified_ledger` set
- [ ] Test `reject_asset` — verifier-only, status transitions
- [ ] Test `freeze_asset` / `unfreeze_asset` — admin-only, only on `Verified`
- [ ] Test `update_appraisal` — resets to `Pending`, clears verifier
- [ ] Test `retire_asset` — owner-only, only from `Verified` or `Rejected`
- [ ] Test `is_verified` — returns correct bool per status
- [ ] Test unauthorized callers panic for all auth-gated functions

**Acceptance Criteria:**
- All tests pass with `cargo test`
- Each function has at least one happy-path and one error-path test
- Tests use `soroban_sdk::testutils::{Address as _, Ledger as _}`

**Complexity:** Medium — 150 pts

---

## Issue #2 — Full Test Suite: Fractional Contract

**Title:** `Add full test suite for fractional contract`

**Labels:** `good first issue`, `testing`, `complexity: medium`, `wave: 150pts`

**Description:**
The `fractional` contract needs a complete test suite covering offerings, share purchases, transfers, and dividend distribution/claiming.

**Tasks:**
- [ ] Test `initialize` — admin and fee rate stored, double-init panics
- [ ] Test `create_offering` — owner holds all shares, event emitted
- [ ] Test `purchase_shares` — holdings updated, fee deducted, `Sold` when fully purchased
- [ ] Test `purchase_shares` — panics below `min_purchase`, panics when oversold
- [ ] Test `transfer_shares` — holdings updated for both parties, panics on insufficient shares
- [ ] Test `distribute_dividend` — tokens locked in contract, round counter incremented
- [ ] Test `claim_dividend` — correct payout proportional to holding, double-claim panics
- [ ] Test `close_offering` — owner-only, only on `Active`
- [ ] Test `get_holding` returns 0 for unknown holder

**Acceptance Criteria:**
- All tests pass with `cargo test`
- Dividend math verified: `payout == total_amount * holding / total_shares`
- Fee math verified: `fee == price * fee_rate / 10_000`

**Complexity:** Medium — 150 pts

---

## Issue #3 — Full Test Suite: Marketplace Contract

**Title:** `Add full test suite for marketplace contract`

**Labels:** `good first issue`, `testing`, `complexity: medium`, `wave: 150pts`

**Description:**
The `marketplace` contract needs a complete test suite covering fixed-price listings, auctions, bidding, settlement, and cancellation.

**Tasks:**
- [ ] Test `create_listing` — both `FixedPrice` and `Auction` types, event emitted
- [ ] Test `buy_now` — tokens transferred with fee split, status set to `Sold`
- [ ] Test `buy_now` — panics on auction listing, panics after deadline
- [ ] Test `place_bid` — bid locked in contract, previous bidder refunded
- [ ] Test `place_bid` — panics on fixed-price listing, panics on low bid, panics after deadline
- [ ] Test `settle_auction` — winner pays seller minus fee, `Sold` status
- [ ] Test `settle_auction` — no bids → `Expired` status
- [ ] Test `settle_auction` — panics before deadline
- [ ] Test `cancel_listing` — seller-only, refunds highest bidder on auction
- [ ] Test `expire_listing` — anyone can call after deadline on fixed-price

**Acceptance Criteria:**
- All tests pass with `cargo test`
- Bid refund verified: previous highest bidder receives exact previous bid amount
- Fee split verified for both `buy_now` and `settle_auction`

**Complexity:** Medium — 150 pts

---

## Issue #4 — KYC / Compliance Flag for Asset Registry

**Title:** `Add KYC/compliance flag and allowlist to asset_registry`

**Labels:** `enhancement`, `compliance`, `complexity: high`, `wave: 200pts`

**Description:**
Regulated RWA markets require KYC verification before users can register or hold assets. Add an optional KYC allowlist to the asset registry so the admin can gate `register_asset` to approved addresses only.

**Tasks:**
- [ ] Add `DataKey::KycEnabled` (bool) and `DataKey::KycApproved(Address)` (bool) storage keys
- [ ] Add `set_kyc_enabled(env, enabled: bool)` — admin only
- [ ] Add `approve_kyc(env, user: Address)` — admin only, emits `kyc_approved` event
- [ ] Add `revoke_kyc(env, user: Address)` — admin only, emits `kyc_revoked` event
- [ ] Add `is_kyc_approved(env, user: Address) -> bool` view function
- [ ] Gate `register_asset` — if KYC is enabled, caller must be KYC-approved
- [ ] Add tests for all new functions and the gated `register_asset` path

**Acceptance Criteria:**
- When `kyc_enabled = false`, `register_asset` works for any address (backward compatible)
- When `kyc_enabled = true`, non-approved addresses cannot register assets
- All new functions emit events
- Tests cover both enabled and disabled KYC modes

**Complexity:** High — 200 pts

---

## Issue #5 — Share Buyback Mechanism for Fractional Contract

**Title:** `Add share buyback mechanism to fractional contract`

**Labels:** `enhancement`, `complexity: high`, `wave: 200pts`

**Description:**
Asset owners should be able to buy back outstanding shares from holders at a set price, enabling full asset reclamation or liquidity events.

**Tasks:**
- [ ] Add `DataKey::Buyback(asset_id)` storing a `BuybackOffer { price_per_share, token, active }`
- [ ] Add `create_buyback(env, owner, asset_id, price_per_share, token)` — owner only, emits event
- [ ] Add `cancel_buyback(env, owner, asset_id)` — owner only
- [ ] Add `accept_buyback(env, holder, asset_id, shares)` — transfers shares to owner, pays holder
- [ ] Validate holder has sufficient shares before accepting
- [ ] Emit events on create, cancel, and accept
- [ ] Add tests for full buyback lifecycle

**Acceptance Criteria:**
- Owner can create and cancel a buyback offer
- Holders can sell any number of their shares (up to their holding) to the buyback
- Payment is `shares * price_per_share` transferred from owner to holder
- Shares are transferred from holder to owner via existing holding storage

**Complexity:** High — 200 pts

---

## Issue #6 — Reserve Price and Bid Increment for Auctions

**Title:** `Add reserve price and minimum bid increment to marketplace auctions`

**Labels:** `enhancement`, `complexity: high`, `wave: 200pts`

**Description:**
Production auctions need a reserve price (minimum winning bid) and a minimum bid increment to prevent bid sniping and ensure meaningful price discovery.

**Tasks:**
- [ ] Add `reserve_price: i128` and `min_bid_increment: i128` fields to `Listing`
- [ ] Update `create_listing` to accept these new fields (use `0` as "no reserve / no increment")
- [ ] In `place_bid`: enforce `amount >= highest_bid + min_bid_increment` when `min_bid_increment > 0`
- [ ] In `settle_auction`: if `highest_bid < reserve_price`, refund highest bidder and mark `Expired`
- [ ] Add tests for reserve price not met, bid increment enforcement, and normal auction with both set

**Acceptance Criteria:**
- Bids below `highest_bid + min_bid_increment` are rejected
- Auctions where `highest_bid < reserve_price` expire and refund the highest bidder
- Existing auctions with `reserve_price = 0` and `min_bid_increment = 0` behave identically to current behavior

**Complexity:** High — 200 pts

---

## Issue #7 — Full Ownership Transfer for Asset Registry

**Title:** `Add asset ownership transfer to asset_registry`

**Labels:** `enhancement`, `complexity: medium`, `wave: 150pts`

**Description:**
Asset owners need to be able to transfer ownership of a registered asset to another address (e.g., after a full share buyback or legal transfer).

**Tasks:**
- [ ] Add `transfer_ownership(env, asset_id, new_owner: Address)` — current owner auth required
- [ ] Update `RwaAsset.owner` to `new_owner`
- [ ] Emit `ownership_transferred` event with `(asset_id, old_owner, new_owner)`
- [ ] Only allow transfer on `Verified` or `Pending` assets (not `Frozen`, `Retired`, `Rejected`)
- [ ] Add tests: successful transfer, unauthorized caller panics, invalid status panics

**Acceptance Criteria:**
- After transfer, `get_asset` returns the new owner
- Old owner can no longer call owner-gated functions (`retire_asset`, `update_appraisal`)
- New owner can call all owner-gated functions

**Complexity:** Medium — 150 pts

---

## Issue #8 — Listing Search Metadata and Price Update

**Title:** `Add listing metadata tags and price update to marketplace`

**Labels:** `enhancement`, `complexity: medium`, `wave: 150pts`

**Description:**
Marketplace UIs need to filter listings by asset type and allow sellers to update fixed-price listings before they sell.

**Tasks:**
- [ ] Add `tags: soroban_sdk::Vec<soroban_sdk::Symbol>` field to `Listing` for off-chain indexing
- [ ] Update `create_listing` to accept `tags`
- [ ] Add `update_price(env, seller, listing_id, new_price: i128)` — seller auth, only on `Active` `FixedPrice` listings
- [ ] Emit `price_updated` event with `(listing_id, old_price, new_price)`
- [ ] Add tests for `update_price` happy path, unauthorized caller, wrong listing type, and inactive listing

**Acceptance Criteria:**
- `update_price` updates `listing.price` and emits the event
- `update_price` panics if caller is not the seller
- `update_price` panics on `Auction` listings
- Tags are stored and returned in `get_listing`

**Complexity:** Medium — 150 pts

---

## Issue #9 — Doc Comments for All Contracts

**Title:** `Add rustdoc comments to all public types and functions`

**Labels:** `documentation`, `good first issue`, `complexity: trivial`, `wave: 100pts`

**Description:**
All three contracts are missing `///` doc comments on public structs, enums, and functions. Good documentation is essential for contributors and integrators.

**Tasks:**
- [ ] Add `///` doc comments to every `pub fn` in `asset_registry/src/lib.rs`
- [ ] Add `///` doc comments to every `pub fn` in `fractional/src/lib.rs`
- [ ] Add `///` doc comments to every `pub fn` in `marketplace/src/lib.rs`
- [ ] Add `///` doc comments to all `#[contracttype]` structs and enums
- [ ] Verify `cargo doc --no-deps` builds without warnings

**Acceptance Criteria:**
- `cargo doc --no-deps` produces zero warnings
- Every public function has at least a one-line summary
- Panicking conditions are documented with `# Panics` sections

**Complexity:** Trivial — 100 pts

---

## Issue #10 — Testnet Deploy Script with Example Asset and Offering

**Title:** `Add testnet deploy script with end-to-end example`

**Labels:** `tooling`, `documentation`, `complexity: medium`, `wave: 150pts`

**Description:**
New contributors and integrators need a working shell script that deploys all three contracts to Stellar testnet and runs a complete end-to-end flow: register an asset, create a fractional offering, and list shares on the marketplace.

**Tasks:**
- [ ] Create `scripts/deploy_testnet.sh` that:
  - Generates or reuses a `stellar keys` identity
  - Funds the account via Friendbot
  - Deploys and initializes all three contracts
  - Registers a sample real estate asset
  - Creates a fractional offering with 1000 shares at 10 USDC each
  - Creates a fixed-price marketplace listing for 100 shares
  - Prints all contract IDs and transaction hashes
- [ ] Add a `scripts/README.md` explaining prerequisites and usage
- [ ] Test the script end-to-end on Stellar testnet

**Acceptance Criteria:**
- Script runs to completion on a fresh testnet account
- All contract IDs are printed at the end
- Script is idempotent (re-running with the same identity does not crash)
- `scripts/README.md` documents every step and required env vars

**Complexity:** Medium — 150 pts
