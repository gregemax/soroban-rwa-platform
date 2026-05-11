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

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Events, MockAuth, MockAuthInvoke},
        token, IntoVal, Symbol, TryIntoVal,
    };

    const ASSET_ID: u64 = 7;
    const TOTAL_SHARES: i128 = 1_000;
    const PRICE_PER_SHARE: i128 = 10;
    const MIN_PURCHASE: i128 = 10;
    const FEE_RATE_BPS: u32 = 100;

    struct Setup {
        env: Env,
        contract_id: Address,
        admin: Address,
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
        let token = env.register_stellar_asset_contract_v2(token_admin).address();
        client.initialize(&admin, &FEE_RATE_BPS);

        Setup {
            env,
            contract_id,
            admin,
            token,
        }
    }

    fn setup_without_mock_all_auths() -> Setup {
        let env = Env::default();
        let contract_id = env.register_contract(None, Fractional);
        let client = FractionalClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let token_admin = Address::generate(&env);
        let token = env.register_stellar_asset_contract_v2(token_admin).address();
        client.initialize(&admin, &FEE_RATE_BPS);

        Setup {
            env,
            contract_id,
            admin,
            token,
        }
    }

    fn token_client<'a>(env: &'a Env, token: &'a Address) -> token::Client<'a> {
        token::Client::new(env, token)
    }

    fn asset_client<'a>(env: &'a Env, token: &'a Address) -> token::StellarAssetClient<'a> {
        token::StellarAssetClient::new(env, token)
    }

    fn create_offering(
        client: &FractionalClient,
        owner: &Address,
        token: &Address,
        total_shares: i128,
        price_per_share: i128,
        min_purchase: i128,
    ) -> u64 {
        client.create_offering(
            owner,
            &ASSET_ID,
            token,
            &total_shares,
            &price_per_share,
            &min_purchase,
        )
    }

    fn create_offering_with_owner_auth(
        setup: &Setup,
        owner: &Address,
        total_shares: i128,
        price_per_share: i128,
        min_purchase: i128,
    ) -> u64 {
        client(setup)
            .mock_auths(&[MockAuth {
                address: owner,
                invoke: &MockAuthInvoke {
                    contract: &setup.contract_id,
                    fn_name: "create_offering",
                    args: (
                        owner,
                        ASSET_ID,
                        setup.token.clone(),
                        total_shares,
                        price_per_share,
                        min_purchase,
                    )
                        .into_val(&setup.env),
                    sub_invokes: &[],
                },
            }])
            .create_offering(
                owner,
                &ASSET_ID,
                &setup.token,
                &total_shares,
                &price_per_share,
                &min_purchase,
            )
    }

    fn close_offering_with_auth(setup: &Setup, signer: &Address, offering_id: u64) {
        client(setup)
            .mock_auths(&[MockAuth {
                address: signer,
                invoke: &MockAuthInvoke {
                    contract: &setup.contract_id,
                    fn_name: "close_offering",
                    args: (offering_id,).into_val(&setup.env),
                    sub_invokes: &[],
                },
            }])
            .close_offering(&offering_id);
    }

    #[test]
    fn initialize_stores_admin_and_fee_rate() {
        let setup = setup();
        let stored: (Address, u32) = setup.env.as_contract(&setup.contract_id, || {
            let admin: Address = setup
                .env
                .storage()
                .instance()
                .get(&DataKey::Admin)
                .unwrap();
            let fee_rate: u32 = setup
                .env
                .storage()
                .instance()
                .get(&DataKey::FeeRate)
                .unwrap();
            (admin, fee_rate)
        });

        assert!(stored.0 == setup.admin);
        assert!(stored.1 == FEE_RATE_BPS);
    }

    #[test]
    #[should_panic]
    fn initialize_rejects_double_init() {
        let setup = setup();
        let other_admin = Address::generate(&setup.env);

        client(&setup).initialize(&other_admin, &0u32);
    }

    #[test]
    fn create_offering_stores_offering_holding_and_event() {
        let setup = setup();
        let owner = Address::generate(&setup.env);

        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );

        let offering = client(&setup).get_offering(&offering_id);
        assert!(offering.owner == owner);
        assert!(offering.asset_id == ASSET_ID);
        assert!(offering.total_shares == TOTAL_SHARES);
        assert!(offering.price_per_share == PRICE_PER_SHARE);
        assert!(offering.min_purchase == MIN_PURCHASE);
        assert!(offering.status == OfferingStatus::Active);
        assert!(client(&setup).get_holding(&ASSET_ID, &owner) == TOTAL_SHARES);

        let events = setup.env.events().all();
        let (_contract, topics, data) = events.get(events.len() - 1).unwrap();
        let topic: Symbol = topics.get(0).unwrap().try_into_val(&setup.env).unwrap();
        let event_data: (u64, Address, u64) = data.try_into_val(&setup.env).unwrap();
        assert!(topic == symbol_short!("offering"));
        assert!(event_data == (offering_id, owner, ASSET_ID));
    }

    #[test]
    fn purchase_shares_updates_holdings_fees_and_sold_status() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let buyer = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&buyer, &2_000i128);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            100i128,
            PRICE_PER_SHARE,
            1i128,
        );

        client(&setup).purchase_shares(&buyer, &offering_id, &100i128);

        let cost = 100i128 * PRICE_PER_SHARE;
        let fee = cost * FEE_RATE_BPS as i128 / 10_000;
        assert!(fee == 10i128);
        assert!(token_client(&setup.env, &setup.token).balance(&owner) == cost - fee);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.admin) == fee);
        assert!(client(&setup).get_holding(&ASSET_ID, &owner) == 0);
        assert!(client(&setup).get_holding(&ASSET_ID, &buyer) == 100);
        assert!(client(&setup).get_offering(&offering_id).status == OfferingStatus::Sold);
    }

    #[test]
    #[should_panic]
    fn purchase_shares_rejects_below_minimum() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let buyer = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&buyer, &2_000i128);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );

        client(&setup).purchase_shares(&buyer, &offering_id, &1i128);
    }

    #[test]
    #[should_panic]
    fn purchase_shares_rejects_oversell() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let buyer = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&buyer, &20_000i128);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            100i128,
            PRICE_PER_SHARE,
            1i128,
        );

        client(&setup).purchase_shares(&buyer, &offering_id, &101i128);
    }

    #[test]
    fn transfer_shares_updates_sender_and_receiver_holdings() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let recipient = Address::generate(&setup.env);
        create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );

        client(&setup).transfer_shares(&owner, &recipient, &ASSET_ID, &125i128);

        assert!(client(&setup).get_holding(&ASSET_ID, &owner) == 875);
        assert!(client(&setup).get_holding(&ASSET_ID, &recipient) == 125);
    }

    #[test]
    #[should_panic]
    fn transfer_shares_rejects_insufficient_holding() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let recipient = Address::generate(&setup.env);

        client(&setup).transfer_shares(&owner, &recipient, &ASSET_ID, &1i128);
    }

    #[test]
    fn distribute_dividend_locks_tokens_and_increments_round() {
        let setup = setup();
        let distributor = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&distributor, &5_000i128);

        let round = client(&setup).distribute_dividend(
            &distributor,
            &ASSET_ID,
            &1_000i128,
            &TOTAL_SHARES,
            &setup.token,
        );
        let next_round = client(&setup).distribute_dividend(
            &distributor,
            &ASSET_ID,
            &500i128,
            &TOTAL_SHARES,
            &setup.token,
        );

        assert!(round == 0);
        assert!(next_round == 1);
        assert!(
            token_client(&setup.env, &setup.token)
                .balance(&setup.contract_id)
                == 1_500i128
        );
        let info = client(&setup).get_dividend_round(&ASSET_ID, &round);
        assert!(info.total_amount == 1_000i128);
        assert!(info.total_shares == TOTAL_SHARES);
    }

    #[test]
    fn claim_dividend_pays_proportional_holding() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let buyer = Address::generate(&setup.env);
        let distributor = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&buyer, &5_000i128);
        asset_client(&setup.env, &setup.token).mint(&distributor, &5_000i128);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );
        client(&setup).purchase_shares(&buyer, &offering_id, &100i128);
        let round = client(&setup).distribute_dividend(
            &distributor,
            &ASSET_ID,
            &1_000i128,
            &TOTAL_SHARES,
            &setup.token,
        );

        let before = token_client(&setup.env, &setup.token).balance(&buyer);
        client(&setup).claim_dividend(&buyer, &ASSET_ID, &round);
        let after = token_client(&setup.env, &setup.token).balance(&buyer);

        let payout = 1_000i128 * 100i128 / TOTAL_SHARES;
        assert!(payout == 100i128);
        assert!(after - before == payout);
    }

    #[test]
    #[should_panic]
    fn claim_dividend_rejects_double_claim() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let buyer = Address::generate(&setup.env);
        let distributor = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&buyer, &5_000i128);
        asset_client(&setup.env, &setup.token).mint(&distributor, &5_000i128);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );
        client(&setup).purchase_shares(&buyer, &offering_id, &100i128);
        let round = client(&setup).distribute_dividend(
            &distributor,
            &ASSET_ID,
            &1_000i128,
            &TOTAL_SHARES,
            &setup.token,
        );

        client(&setup).claim_dividend(&buyer, &ASSET_ID, &round);
        client(&setup).claim_dividend(&buyer, &ASSET_ID, &round);
    }

    #[test]
    fn close_offering_sets_closed_status() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );

        client(&setup).close_offering(&offering_id);

        assert!(client(&setup).get_offering(&offering_id).status == OfferingStatus::Closed);
    }

    #[test]
    #[should_panic]
    fn close_offering_requires_owner_auth() {
        let setup = setup_without_mock_all_auths();
        let owner = Address::generate(&setup.env);
        let other = Address::generate(&setup.env);
        let offering_id = create_offering_with_owner_auth(
            &setup,
            &owner,
            TOTAL_SHARES,
            PRICE_PER_SHARE,
            MIN_PURCHASE,
        );

        close_offering_with_auth(&setup, &other, offering_id);
    }

    #[test]
    #[should_panic]
    fn close_offering_rejects_inactive_offering() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        let buyer = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&buyer, &2_000i128);
        let offering_id = create_offering(
            &client(&setup),
            &owner,
            &setup.token,
            100i128,
            PRICE_PER_SHARE,
            1i128,
        );
        client(&setup).purchase_shares(&buyer, &offering_id, &100i128);

        client(&setup).close_offering(&offering_id);
    }

    #[test]
    fn get_holding_returns_zero_for_unknown_holder() {
        let setup = setup();
        let holder = Address::generate(&setup.env);

        assert!(client(&setup).get_holding(&ASSET_ID, &holder) == 0);
    }
}
