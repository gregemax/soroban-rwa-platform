#![no_std]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env,
};

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    FeeRate,          // basis points (e.g. 100 = 1%)
    Offering(u64),
    OfferingCount,
    Holding(u64, Address),          // (asset_id, holder)
    DividendRound(u64),             // asset_id -> current round
    DividendInfo(u64, u32),         // (asset_id, round) -> DividendRound
    DividendClaim(u64, u32, Address), // (asset_id, round, holder) -> bool
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum OfferingStatus {
    Active,
    Sold,
    Closed,
}

#[contracttype]
#[derive(Clone)]
pub struct FractionalOffering {
    pub asset_id: u64,
    pub owner: Address,
    pub token: Address,
    pub total_shares: i128,
    pub shares_sold: i128,
    pub price_per_share: i128,
    pub min_purchase: i128,
    pub status: OfferingStatus,
}

#[contracttype]
#[derive(Clone)]
pub struct DividendRound {
    pub asset_id: u64,
    pub round: u32,
    pub total_amount: i128,
    pub total_shares: i128,
    pub token: Address,
    pub created_ledger: u32,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct Fractional;

#[contractimpl]
impl Fractional {
    pub fn initialize(env: Env, admin: Address, fee_rate_bps: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::FeeRate, &fee_rate_bps);
        env.storage().instance().set(&DataKey::OfferingCount, &0u64);
    }

    /// Create a new fractional offering. Owner receives all shares as holding.
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
    pub fn purchase_shares(
        env: Env,
        buyer: Address,
        offering_id: u64,
        shares: i128,
    ) {
        buyer.require_auth();
        let mut offering = Self::load_offering(&env, offering_id);
        assert!(offering.status == OfferingStatus::Active, "not active");
        assert!(shares >= offering.min_purchase, "below min purchase");
        let available = offering.total_shares - offering.shares_sold;
        assert!(shares <= available, "insufficient shares");

        let cost = shares * offering.price_per_share;
        let fee_rate: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeRate)
            .unwrap_or(0);
        let fee = cost * fee_rate as i128 / 10_000;
        let seller_amount = cost - fee;

        let tok = token::Client::new(&env, &offering.token);
        tok.transfer(&buyer, &offering.owner, &seller_amount);
        if fee > 0 {
            let admin: Address = env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .unwrap();
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
    pub fn transfer_shares(
        env: Env,
        from: Address,
        to: Address,
        asset_id: u64,
        shares: i128,
    ) {
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
        env.events()
            .publish((symbol_short!("dividend"),), (asset_id, round, total_amount));
        round
    }

    /// Claim dividend for a specific round.
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
        env.events()
            .publish((symbol_short!("claimed"),), (asset_id, round, holder, payout));
    }

    /// Close an offering (owner only).
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

    pub fn get_offering(env: Env, offering_id: u64) -> FractionalOffering {
        Self::load_offering(&env, offering_id)
    }

    pub fn get_holding(env: Env, asset_id: u64, holder: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Holding(asset_id, holder))
            .unwrap_or(0)
    }

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
