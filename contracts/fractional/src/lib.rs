#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, token, Address, Env};

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
/// Storage keys used by the fractional ownership contract.
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Platform fee rate in basis points.
    FeeRate, // basis points (e.g. 100 = 1%)
    /// Fractional offering keyed by offering id.
    Offering(u64),
    /// Monotonic counter used to assign offering ids.
    OfferingCount,
    /// Share balance for a holder in an asset.
    Holding(u64, Address), // (asset_id, holder)
    /// Current dividend round counter for an asset.
    DividendRound(u64), // asset_id -> current round
    /// Dividend round metadata keyed by asset id and round.
    DividendInfo(u64, u32), // (asset_id, round) -> DividendRound
    /// Dividend claim flag keyed by asset id, round, and holder.
    DividendClaim(u64, u32, Address), // (asset_id, round, holder) -> bool
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
/// Lifecycle state for a fractional share offering.
pub enum OfferingStatus {
    /// The offering accepts share purchases.
    Active,
    /// All shares have been purchased.
    Sold,
    /// The owner has manually closed the offering.
    Closed,
}

#[contracttype]
#[derive(Clone)]
/// Fractional share offering for a registered asset.
pub struct FractionalOffering {
    /// Asset represented by this offering.
    pub asset_id: u64,
    /// Seller and initial holder of all shares.
    pub owner: Address,
    /// Token used to price and buy shares.
    pub token: Address,
    /// Total shares available in the offering.
    pub total_shares: i128,
    /// Number of shares sold so far.
    pub shares_sold: i128,
    /// Price per share in the configured token.
    pub price_per_share: i128,
    /// Minimum shares a buyer must purchase per transaction.
    pub min_purchase: i128,
    /// Current offering lifecycle state.
    pub status: OfferingStatus,
}

#[contracttype]
#[derive(Clone)]
/// Dividend distribution round for holders of a fractional asset.
pub struct DividendRound {
    /// Asset receiving the dividend.
    pub asset_id: u64,
    /// Sequential round number for the asset.
    pub round: u32,
    /// Total token amount available for this round.
    pub total_amount: i128,
    /// Total shares used to calculate proportional payouts.
    pub total_shares: i128,
    /// Token distributed to holders.
    pub token: Address,
    /// Ledger sequence when the round was created.
    pub created_ledger: u32,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
/// Contract for fractional offerings, share transfers, and dividends.
pub struct Fractional;

#[contractimpl]
impl Fractional {
    /// Initialize the fractional contract with an admin and fee rate.
    ///
    /// # Panics
    ///
    /// Panics if the contract has already been initialized.
    pub fn initialize(env: Env, admin: Address, fee_rate_bps: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage()
            .instance()
            .set(&DataKey::FeeRate, &fee_rate_bps);
        env.storage().instance().set(&DataKey::OfferingCount, &0u64);
    }

    /// Create a new fractional offering. Owner receives all shares as holding.
    ///
    /// # Panics
    ///
    /// Panics if the owner does not authorize the call.
    pub fn create_offering(
        env: Env,
        owner: Address,
        asset_id: u64,
        token: Address,
        total_shares: i128,
        price_per_share: i128,
        min_purchase: i128,
    ) -> u64 {
        owner.require_auth();
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::OfferingCount)
            .unwrap_or(0);
        let offering = FractionalOffering {
            asset_id,
            owner: owner.clone(),
            token,
            total_shares,
            shares_sold: 0,
            price_per_share,
            min_purchase,
            status: OfferingStatus::Active,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Offering(id), &offering);
        // Owner holds all shares initially
        env.storage()
            .persistent()
            .set(&DataKey::Holding(asset_id, owner.clone()), &total_shares);
        env.storage()
            .instance()
            .set(&DataKey::OfferingCount, &(id + 1));
        env.events()
            .publish((symbol_short!("offering"),), (id, owner, asset_id));
        id
    }

    /// Purchase shares from an active offering.
    ///
    /// # Panics
    ///
    /// Panics if the buyer does not authorize the call, the offering does not
    /// exist, is not active, the purchase is below the minimum, or insufficient
    /// shares remain.
    pub fn purchase_shares(env: Env, buyer: Address, offering_id: u64, shares: i128) {
        buyer.require_auth();
        let mut offering = Self::load_offering(&env, offering_id);
        assert!(offering.status == OfferingStatus::Active, "not active");
        assert!(shares >= offering.min_purchase, "below min purchase");
        let available = offering.total_shares - offering.shares_sold;
        assert!(shares <= available, "insufficient shares");

        let cost = shares * offering.price_per_share;
        let fee_rate: u32 = env.storage().instance().get(&DataKey::FeeRate).unwrap_or(0);
        let fee = cost * fee_rate as i128 / 10_000;
        let seller_amount = cost - fee;

        let tok = token::Client::new(&env, &offering.token);
        tok.transfer(&buyer, &offering.owner, &seller_amount);
        if fee > 0 {
            let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
            tok.transfer(&buyer, &admin, &fee);
        }

        // Update holdings
        let owner_holding: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Holding(offering.asset_id, offering.owner.clone()))
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::Holding(offering.asset_id, offering.owner.clone()),
            &(owner_holding - shares),
        );
        let buyer_holding: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Holding(offering.asset_id, buyer.clone()))
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::Holding(offering.asset_id, buyer.clone()),
            &(buyer_holding + shares),
        );

        offering.shares_sold += shares;
        if offering.shares_sold == offering.total_shares {
            offering.status = OfferingStatus::Sold;
        }
        env.storage()
            .persistent()
            .set(&DataKey::Offering(offering_id), &offering);
        env.events()
            .publish((symbol_short!("purchased"),), (offering_id, buyer, shares));
    }

    /// Transfer shares between holders.
    ///
    /// # Panics
    ///
    /// Panics if the sender does not authorize the call or does not have enough
    /// shares.
    pub fn transfer_shares(env: Env, from: Address, to: Address, asset_id: u64, shares: i128) {
        from.require_auth();
        let from_holding: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Holding(asset_id, from.clone()))
            .unwrap_or(0);
        assert!(from_holding >= shares, "insufficient shares");
        env.storage().persistent().set(
            &DataKey::Holding(asset_id, from.clone()),
            &(from_holding - shares),
        );
        let to_holding: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Holding(asset_id, to.clone()))
            .unwrap_or(0);
        env.storage().persistent().set(
            &DataKey::Holding(asset_id, to.clone()),
            &(to_holding + shares),
        );
        env.events()
            .publish((symbol_short!("transfer"),), (asset_id, from, to, shares));
    }

    /// Distribute a dividend round. Caller transfers total_amount to contract first.
    ///
    /// # Panics
    ///
    /// Panics if the distributor does not authorize the call or the token
    /// transfer fails.
    pub fn distribute_dividend(
        env: Env,
        distributor: Address,
        asset_id: u64,
        total_amount: i128,
        total_shares: i128,
        token: Address,
    ) -> u32 {
        distributor.require_auth();
        let tok = token::Client::new(&env, &token);
        tok.transfer(&distributor, &env.current_contract_address(), &total_amount);

        let round: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::DividendRound(asset_id))
            .unwrap_or(0);
        let info = DividendRound {
            asset_id,
            round,
            total_amount,
            total_shares,
            token,
            created_ledger: env.ledger().sequence(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::DividendInfo(asset_id, round), &info);
        env.storage()
            .persistent()
            .set(&DataKey::DividendRound(asset_id), &(round + 1));
        env.events().publish(
            (symbol_short!("dividend"),),
            (asset_id, round, total_amount),
        );
        round
    }

    /// Claim dividend for a specific round.
    ///
    /// # Panics
    ///
    /// Panics if the holder does not authorize the call, the dividend was
    /// already claimed, the round does not exist, or the holder has no shares.
    pub fn claim_dividend(env: Env, holder: Address, asset_id: u64, round: u32) {
        holder.require_auth();
        let claimed: bool = env
            .storage()
            .persistent()
            .get(&DataKey::DividendClaim(asset_id, round, holder.clone()))
            .unwrap_or(false);
        assert!(!claimed, "already claimed");

        let info: DividendRound = env
            .storage()
            .persistent()
            .get(&DataKey::DividendInfo(asset_id, round))
            .expect("round not found");
        let holding: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::Holding(asset_id, holder.clone()))
            .unwrap_or(0);
        assert!(holding > 0, "no shares");

        let payout = info.total_amount * holding / info.total_shares;
        // Zero claim before transfer (reentrancy guard)
        env.storage().persistent().set(
            &DataKey::DividendClaim(asset_id, round, holder.clone()),
            &true,
        );
        let tok = token::Client::new(&env, &info.token);
        tok.transfer(&env.current_contract_address(), &holder, &payout);
        env.events().publish(
            (symbol_short!("claimed"),),
            (asset_id, round, holder, payout),
        );
    }

    /// Close an offering (owner only).
    ///
    /// # Panics
    ///
    /// Panics if the offering does not exist, the owner does not authorize the
    /// call, or the offering is not active.
    pub fn close_offering(env: Env, offering_id: u64) {
        let mut offering = Self::load_offering(&env, offering_id);
        offering.owner.require_auth();
        assert!(offering.status == OfferingStatus::Active, "not active");
        offering.status = OfferingStatus::Closed;
        env.storage()
            .persistent()
            .set(&DataKey::Offering(offering_id), &offering);
        env.events()
            .publish((symbol_short!("closed"),), (offering_id,));
    }

    /// Return an offering by id.
    ///
    /// # Panics
    ///
    /// Panics if the offering does not exist.
    pub fn get_offering(env: Env, offering_id: u64) -> FractionalOffering {
        Self::load_offering(&env, offering_id)
    }

    /// Return the share balance for a holder in an asset.
    pub fn get_holding(env: Env, asset_id: u64, holder: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Holding(asset_id, holder))
            .unwrap_or(0)
    }

    /// Return dividend metadata for an asset round.
    ///
    /// # Panics
    ///
    /// Panics if the dividend round does not exist.
    pub fn get_dividend_round(env: Env, asset_id: u64, round: u32) -> DividendRound {
        env.storage()
            .persistent()
            .get(&DataKey::DividendInfo(asset_id, round))
            .expect("round not found")
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn load_offering(env: &Env, offering_id: u64) -> FractionalOffering {
        env.storage()
            .persistent()
            .get(&DataKey::Offering(offering_id))
            .expect("offering not found")
    }
}
