#![no_std]
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, token, Address, Env};

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    FeeRate, // basis points (e.g. 100 = 1%)
    Offering(u64),
    OfferingCount,
    AssetOwner(u64),                  // asset_id -> original owner
    Holding(u64, Address),            // (asset_id, holder)
    Buyback(u64),                     // asset_id -> BuybackOffer
    DividendRound(u64),               // asset_id -> current round
    DividendInfo(u64, u32),           // (asset_id, round) -> DividendRound
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

#[contracttype]
#[derive(Clone, PartialEq)]
pub struct BuybackOffer {
    pub owner: Address,
    pub price_per_share: i128,
    pub token: Address,
    pub active: bool,
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
        env.storage()
            .instance()
            .set(&DataKey::FeeRate, &fee_rate_bps);
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
            .persistent()
            .set(&DataKey::AssetOwner(asset_id), &owner);
        env.storage()
            .instance()
            .set(&DataKey::OfferingCount, &(id + 1));
        env.events()
            .publish((symbol_short!("offering"),), (id, owner, asset_id));
        id
    }

    /// Purchase shares from an active offering.
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

    /// Create an active buyback offer for holders to sell shares back.
    pub fn create_buyback(
        env: Env,
        owner: Address,
        asset_id: u64,
        price_per_share: i128,
        token: Address,
    ) {
        owner.require_auth();
        assert!(price_per_share > 0, "invalid price");
        let asset_owner = Self::load_asset_owner(&env, asset_id);
        assert!(asset_owner == owner, "not owner");

        let offer = BuybackOffer {
            owner: owner.clone(),
            price_per_share,
            token,
            active: true,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Buyback(asset_id), &offer);
        env.events().publish(
            (symbol_short!("buyback"),),
            (asset_id, owner, price_per_share),
        );
    }

    /// Cancel an active buyback offer.
    pub fn cancel_buyback(env: Env, owner: Address, asset_id: u64) {
        owner.require_auth();
        let mut offer = Self::load_buyback(&env, asset_id);
        assert!(offer.owner == owner, "not owner");
        assert!(offer.active, "not active");

        offer.active = false;
        env.storage()
            .persistent()
            .set(&DataKey::Buyback(asset_id), &offer);
        env.events()
            .publish((symbol_short!("bb_cancel"),), (asset_id, owner));
    }

    /// Accept an active buyback offer by selling shares back to the asset owner.
    pub fn accept_buyback(env: Env, holder: Address, asset_id: u64, shares: i128) {
        holder.require_auth();
        assert!(shares > 0, "invalid shares");
        let offer = Self::load_buyback(&env, asset_id);
        assert!(offer.active, "not active");

        let holder_holding = Self::load_holding(&env, asset_id, holder.clone());
        assert!(holder_holding >= shares, "insufficient shares");
        let owner_holding = Self::load_holding(&env, asset_id, offer.owner.clone());
        let payment = shares * offer.price_per_share;

        Self::set_holding(&env, asset_id, holder.clone(), holder_holding - shares);
        Self::set_holding(&env, asset_id, offer.owner.clone(), owner_holding + shares);

        let tok = token::Client::new(&env, &offer.token);
        tok.transfer_from(
            &env.current_contract_address(),
            &offer.owner,
            &holder,
            &payment,
        );
        env.events().publish(
            (symbol_short!("bb_accept"),),
            (asset_id, holder, shares, payment),
        );
    }

    pub fn get_offering(env: Env, offering_id: u64) -> FractionalOffering {
        Self::load_offering(&env, offering_id)
    }

    pub fn get_buyback(env: Env, asset_id: u64) -> BuybackOffer {
        Self::load_buyback(&env, asset_id)
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

    fn load_asset_owner(env: &Env, asset_id: u64) -> Address {
        env.storage()
            .persistent()
            .get(&DataKey::AssetOwner(asset_id))
            .expect("asset owner not found")
    }

    fn load_buyback(env: &Env, asset_id: u64) -> BuybackOffer {
        env.storage()
            .persistent()
            .get(&DataKey::Buyback(asset_id))
            .expect("buyback not found")
    }

    fn load_holding(env: &Env, asset_id: u64, holder: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::Holding(asset_id, holder))
            .unwrap_or(0)
    }

    fn set_holding(env: &Env, asset_id: u64, holder: Address, shares: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::Holding(asset_id, holder), &shares);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events},
        token, Symbol, TryIntoVal,
    };

    const ASSET_ID: u64 = 7;
    const TOTAL_SHARES: i128 = 1_000;
    const PRICE_PER_SHARE: i128 = 10;
    const MIN_PURCHASE: i128 = 10;
    const FEE_RATE_BPS: u32 = 100;
    const BUYBACK_PRICE: i128 = 12;

    struct Setup {
        env: Env,
        contract_id: Address,
        token: Address,
    }

    fn client(setup: &Setup) -> FractionalClient<'_> {
        FractionalClient::new(&setup.env, &setup.contract_id)
    }

    fn setup() -> Setup {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, Fractional);
        let client = FractionalClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = env
            .register_stellar_asset_contract_v2(token_admin)
            .address();
        client.initialize(&admin, &FEE_RATE_BPS);

        Setup {
            env,
            contract_id,
            token,
        }
    }

    fn token_client<'a>(env: &'a Env, token: &'a Address) -> token::Client<'a> {
        token::Client::new(env, token)
    }

    fn asset_client<'a>(env: &'a Env, token: &'a Address) -> token::StellarAssetClient<'a> {
        token::StellarAssetClient::new(env, token)
    }

    fn create_offering(setup: &Setup, owner: &Address, total_shares: i128) -> u64 {
        client(setup).create_offering(
            owner,
            &ASSET_ID,
            &setup.token,
            &total_shares,
            &PRICE_PER_SHARE,
            &MIN_PURCHASE,
        )
    }

    fn approve_buyback_spend(setup: &Setup, owner: &Address, amount: i128) {
        token_client(&setup.env, &setup.token).approve(
            owner,
            &setup.contract_id,
            &amount,
            &10_000u32,
        );
    }

    #[test]
    fn create_buyback_stores_offer_and_event() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        create_offering(&setup, &owner, TOTAL_SHARES);

        client(&setup).create_buyback(&owner, &ASSET_ID, &BUYBACK_PRICE, &setup.token);

        let offer = client(&setup).get_buyback(&ASSET_ID);
        assert!(offer.owner == owner);
        assert!(offer.price_per_share == BUYBACK_PRICE);
        assert!(offer.token == setup.token);
        assert!(offer.active);

        let events = setup.env.events().all();
        let (_contract, topics, data) = events.get(events.len() - 1).unwrap();
        let topic: Symbol = topics.get(0).unwrap().try_into_val(&setup.env).unwrap();
        let event_data: (u64, Address, i128) = data.try_into_val(&setup.env).unwrap();
        assert!(topic == symbol_short!("buyback"));
        assert!(event_data == (ASSET_ID, owner, BUYBACK_PRICE));
    }

    #[test]
    fn cancel_buyback_deactivates_offer() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        create_offering(&setup, &owner, TOTAL_SHARES);
        client(&setup).create_buyback(&owner, &ASSET_ID, &BUYBACK_PRICE, &setup.token);

        client(&setup).cancel_buyback(&owner, &ASSET_ID);

        assert!(!client(&setup).get_buyback(&ASSET_ID).active);
    }

    #[test]
    fn accept_buyback_transfers_shares_and_payment() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let holder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&owner, &5_000i128);
        asset_client(&setup.env, &setup.token).mint(&holder, &5_000i128);
        let offering_id = create_offering(&setup, &owner, TOTAL_SHARES);
        client(&setup).purchase_shares(&holder, &offering_id, &100i128);
        client(&setup).create_buyback(&owner, &ASSET_ID, &BUYBACK_PRICE, &setup.token);
        approve_buyback_spend(&setup, &owner, 10_000i128);

        let owner_balance_before = token_client(&setup.env, &setup.token).balance(&owner);
        let holder_balance_before = token_client(&setup.env, &setup.token).balance(&holder);
        client(&setup).accept_buyback(&holder, &ASSET_ID, &40i128);

        let payment = 40i128 * BUYBACK_PRICE;
        assert!(client(&setup).get_holding(&ASSET_ID, &holder) == 60);
        assert!(client(&setup).get_holding(&ASSET_ID, &owner) == 940);
        assert!(
            token_client(&setup.env, &setup.token).balance(&holder)
                == holder_balance_before + payment
        );
        assert!(
            token_client(&setup.env, &setup.token).balance(&owner)
                == owner_balance_before - payment
        );
    }

    #[test]
    fn original_owner_can_buy_back_after_selling_all_shares() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let holder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&owner, &5_000i128);
        asset_client(&setup.env, &setup.token).mint(&holder, &5_000i128);
        let offering_id = client(&setup).create_offering(
            &owner,
            &ASSET_ID,
            &setup.token,
            &100i128,
            &PRICE_PER_SHARE,
            &1i128,
        );
        client(&setup).purchase_shares(&holder, &offering_id, &100i128);
        assert!(client(&setup).get_holding(&ASSET_ID, &owner) == 0);
        client(&setup).create_buyback(&owner, &ASSET_ID, &BUYBACK_PRICE, &setup.token);
        approve_buyback_spend(&setup, &owner, 2_000i128);

        client(&setup).accept_buyback(&holder, &ASSET_ID, &100i128);

        assert!(client(&setup).get_holding(&ASSET_ID, &owner) == 100);
        assert!(client(&setup).get_holding(&ASSET_ID, &holder) == 0);
    }

    #[test]
    #[should_panic]
    fn create_buyback_requires_original_owner() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let other = Address::generate(&setup.env);
        create_offering(&setup, &owner, TOTAL_SHARES);

        client(&setup).create_buyback(&other, &ASSET_ID, &BUYBACK_PRICE, &setup.token);
    }

    #[test]
    #[should_panic]
    fn accept_buyback_rejects_insufficient_shares() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let holder = Address::generate(&setup.env);
        create_offering(&setup, &owner, TOTAL_SHARES);
        client(&setup).create_buyback(&owner, &ASSET_ID, &BUYBACK_PRICE, &setup.token);
        approve_buyback_spend(&setup, &owner, 10_000i128);

        client(&setup).accept_buyback(&holder, &ASSET_ID, &1i128);
    }

    #[test]
    #[should_panic]
    fn accept_buyback_rejects_cancelled_offer() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let holder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&holder, &5_000i128);
        let offering_id = create_offering(&setup, &owner, TOTAL_SHARES);
        client(&setup).purchase_shares(&holder, &offering_id, &100i128);
        client(&setup).create_buyback(&owner, &ASSET_ID, &BUYBACK_PRICE, &setup.token);
        approve_buyback_spend(&setup, &owner, 10_000i128);
        client(&setup).cancel_buyback(&owner, &ASSET_ID);

        client(&setup).accept_buyback(&holder, &ASSET_ID, &1i128);
    }
}
