#![no_std]
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, token, Address, Env,
};

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    FeeRate,        // basis points
    Listing(u64),
    ListingCount,
    Offer(u64, u64), // (listing_id, offer_index)
    OfferCount(u64), // listing_id -> offer count
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum ListingType {
    FixedPrice,
    Auction,
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum ListingStatus {
    Active,
    Sold,
    Expired,
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
pub struct Listing {
    pub seller: Address,
    pub asset_id: u64,
    pub shares: i128,
    pub token: Address,
    pub price: i128,
    pub reserve_price: i128,
    pub min_bid_increment: i128,
    pub highest_bid: i128,
    pub highest_bidder: Option<Address>,
    pub listing_type: ListingType,
    pub deadline_ledger: u32,
    pub status: ListingStatus,
}

#[contracttype]
#[derive(Clone)]
pub struct Offer {
    pub listing_id: u64,
    pub bidder: Address,
    pub amount: i128,
    pub active: bool,
    pub ledger: u32,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
pub struct Marketplace;

#[contractimpl]
impl Marketplace {
    pub fn initialize(env: Env, admin: Address, fee_rate_bps: u32) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::FeeRate, &fee_rate_bps);
        env.storage().instance().set(&DataKey::ListingCount, &0u64);
    }

    /// Create a fixed-price or auction listing.
    pub fn create_listing(
        env: Env,
        seller: Address,
        asset_id: u64,
        shares: i128,
        token: Address,
        price: i128,
        reserve_price: i128,
        min_bid_increment: i128,
        listing_type: ListingType,
        deadline_ledger: u32,
    ) -> u64 {
        seller.require_auth();
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::ListingCount)
            .unwrap_or(0);
        let listing = Listing {
            seller: seller.clone(),
            asset_id,
            shares,
            token,
            price,
            reserve_price,
            min_bid_increment,
            highest_bid: 0,
            highest_bidder: None,
            listing_type,
            deadline_ledger,
            status: ListingStatus::Active,
        };
        env.storage()
            .persistent()
            .set(&DataKey::Listing(id), &listing);
        env.storage()
            .instance()
            .set(&DataKey::ListingCount, &(id + 1));
        env.events()
            .publish((symbol_short!("listed"),), (id, seller, asset_id, shares));
        id
    }

    /// Buy a fixed-price listing immediately.
    pub fn buy_now(env: Env, buyer: Address, listing_id: u64) {
        buyer.require_auth();
        let mut listing = Self::load_listing(&env, listing_id);
        assert!(listing.status == ListingStatus::Active, "not active");
        assert!(listing.listing_type == ListingType::FixedPrice, "not fixed price");
        assert!(
            env.ledger().sequence() <= listing.deadline_ledger,
            "listing expired"
        );

        let fee_rate: u32 = env
            .storage()
            .instance()
            .get(&DataKey::FeeRate)
            .unwrap_or(0);
        let fee = listing.price * fee_rate as i128 / 10_000;
        let seller_amount = listing.price - fee;

        let tok = token::Client::new(&env, &listing.token);
        tok.transfer(&buyer, &listing.seller, &seller_amount);
        if fee > 0 {
            let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
            tok.transfer(&buyer, &admin, &fee);
        }

        listing.status = ListingStatus::Sold;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);
        env.events()
            .publish((symbol_short!("sold"),), (listing_id, buyer, listing.price));
    }

    /// Place a bid on an auction listing. Auto-refunds previous highest bidder.
    pub fn place_bid(env: Env, bidder: Address, listing_id: u64, amount: i128) {
        bidder.require_auth();
        let mut listing = Self::load_listing(&env, listing_id);
        assert!(listing.status == ListingStatus::Active, "not active");
        assert!(listing.listing_type == ListingType::Auction, "not auction");
        assert!(
            env.ledger().sequence() <= listing.deadline_ledger,
            "auction ended"
        );
        if listing.min_bid_increment > 0 {
            assert!(
                amount >= listing.highest_bid + listing.min_bid_increment,
                "bid increment too low"
            );
        } else {
            assert!(amount > listing.highest_bid, "bid too low");
        }

        let tok = token::Client::new(&env, &listing.token);

        // Refund previous highest bidder
        if let Some(ref prev_bidder) = listing.highest_bidder {
            if listing.highest_bid > 0 {
                tok.transfer(
                    &env.current_contract_address(),
                    prev_bidder,
                    &listing.highest_bid,
                );
            }
        }

        // Lock new bid in contract
        tok.transfer(&bidder, &env.current_contract_address(), &amount);

        // Record offer
        let offer_idx: u64 = env
            .storage()
            .persistent()
            .get(&DataKey::OfferCount(listing_id))
            .unwrap_or(0);
        let offer = Offer {
            listing_id,
            bidder: bidder.clone(),
            amount,
            active: true,
            ledger: env.ledger().sequence(),
        };
        env.storage()
            .persistent()
            .set(&DataKey::Offer(listing_id, offer_idx), &offer);
        env.storage()
            .persistent()
            .set(&DataKey::OfferCount(listing_id), &(offer_idx + 1));

        listing.highest_bid = amount;
        listing.highest_bidder = Some(bidder.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);
        env.events()
            .publish((symbol_short!("bid"),), (listing_id, bidder, amount));
    }

    /// Settle an auction after deadline. Anyone can call.
    pub fn settle_auction(env: Env, listing_id: u64) {
        let mut listing = Self::load_listing(&env, listing_id);
        assert!(listing.status == ListingStatus::Active, "not active");
        assert!(listing.listing_type == ListingType::Auction, "not auction");
        assert!(
            env.ledger().sequence() > listing.deadline_ledger,
            "auction not ended"
        );

        if let Some(ref winner) = listing.highest_bidder.clone() {
            let tok = token::Client::new(&env, &listing.token);

            if listing.reserve_price > 0 && listing.highest_bid < listing.reserve_price {
                tok.transfer(&env.current_contract_address(), winner, &listing.highest_bid);
                listing.status = ListingStatus::Expired;
                env.events()
                    .publish((symbol_short!("expired"),), (listing_id,));
                env.storage()
                    .persistent()
                    .set(&DataKey::Listing(listing_id), &listing);
                return;
            }

            let fee_rate: u32 = env
                .storage()
                .instance()
                .get(&DataKey::FeeRate)
                .unwrap_or(0);
            let fee = listing.highest_bid * fee_rate as i128 / 10_000;
            let seller_amount = listing.highest_bid - fee;

            tok.transfer(&env.current_contract_address(), &listing.seller, &seller_amount);
            if fee > 0 {
                let admin: Address = env.storage().instance().get(&DataKey::Admin).unwrap();
                tok.transfer(&env.current_contract_address(), &admin, &fee);
            }
            listing.status = ListingStatus::Sold;
            env.events().publish(
                (symbol_short!("settled"),),
                (listing_id, winner.clone(), listing.highest_bid),
            );
        } else {
            listing.status = ListingStatus::Expired;
            env.events()
                .publish((symbol_short!("expired"),), (listing_id,));
        }
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);
    }

    /// Cancel a listing. Refunds highest bidder on auctions.
    pub fn cancel_listing(env: Env, listing_id: u64) {
        let mut listing = Self::load_listing(&env, listing_id);
        listing.seller.require_auth();
        assert!(listing.status == ListingStatus::Active, "not active");

        if listing.listing_type == ListingType::Auction {
            if let Some(ref prev_bidder) = listing.highest_bidder.clone() {
                if listing.highest_bid > 0 {
                    let tok = token::Client::new(&env, &listing.token);
                    tok.transfer(
                        &env.current_contract_address(),
                        prev_bidder,
                        &listing.highest_bid,
                    );
                }
            }
        }

        listing.status = ListingStatus::Cancelled;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);
        env.events()
            .publish((symbol_short!("cancelled"),), (listing_id,));
    }

    /// Expire a fixed-price listing after deadline. Callable by anyone.
    pub fn expire_listing(env: Env, listing_id: u64) {
        let mut listing = Self::load_listing(&env, listing_id);
        assert!(listing.status == ListingStatus::Active, "not active");
        assert!(listing.listing_type == ListingType::FixedPrice, "not fixed price");
        assert!(
            env.ledger().sequence() > listing.deadline_ledger,
            "not expired yet"
        );
        listing.status = ListingStatus::Expired;
        env.storage()
            .persistent()
            .set(&DataKey::Listing(listing_id), &listing);
        env.events()
            .publish((symbol_short!("expired"),), (listing_id,));
    }

    pub fn get_listing(env: Env, listing_id: u64) -> Listing {
        Self::load_listing(&env, listing_id)
    }

    pub fn get_offer(env: Env, listing_id: u64, offer_idx: u64) -> Offer {
        env.storage()
            .persistent()
            .get(&DataKey::Offer(listing_id, offer_idx))
            .expect("offer not found")
    }

    // ── Helpers ───────────────────────────────────────────────────────────────

    fn load_listing(env: &Env, listing_id: u64) -> Listing {
        env.storage()
            .persistent()
            .get(&DataKey::Listing(listing_id))
            .expect("listing not found")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, Ledger},
        token,
    };

    const ASSET_ID: u64 = 42;
    const SHARES: i128 = 100;
    const PRICE: i128 = 1_000;
    const FIRST_BID: i128 = 700;
    const SECOND_BID: i128 = 900;
    const DEADLINE_LEDGER: u32 = 50;
    const FEE_RATE_BPS: u32 = 100;

    struct Setup {
        env: Env,
        contract_id: Address,
        admin: Address,
        token: Address,
    }

    fn client(setup: &Setup) -> MarketplaceClient<'_> {
        MarketplaceClient::new(&setup.env, &setup.contract_id)
    }

    fn setup() -> Setup {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_sequence_number(10);
        let contract_id = env.register_contract(None, Marketplace);
        let client = MarketplaceClient::new(&env, &contract_id);
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

    fn create_auction(
        setup: &Setup,
        seller: &Address,
        reserve_price: i128,
        min_bid_increment: i128,
    ) -> u64 {
        client(setup).create_listing(
            seller,
            &ASSET_ID,
            &SHARES,
            &setup.token,
            &PRICE,
            &reserve_price,
            &min_bid_increment,
            &ListingType::Auction,
            &DEADLINE_LEDGER,
        )
    }

    #[test]
    fn create_listing_stores_reserve_and_increment() {
        let setup = setup();
        let seller = Address::generate(&setup.env);

        let listing_id = create_auction(&setup, &seller, 800i128, 100i128);

        let listing = client(&setup).get_listing(&listing_id);
        assert!(listing.reserve_price == 800i128);
        assert!(listing.min_bid_increment == 100i128);
        assert!(listing.listing_type == ListingType::Auction);
    }

    #[test]
    #[should_panic]
    fn place_bid_enforces_minimum_increment() {
        let setup = setup();
        let seller = Address::generate(&setup.env);
        let first_bidder = Address::generate(&setup.env);
        let second_bidder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&first_bidder, &FIRST_BID);
        asset_client(&setup.env, &setup.token).mint(&second_bidder, &800i128);
        let listing_id = create_auction(&setup, &seller, 0i128, 100i128);

        client(&setup).place_bid(&first_bidder, &listing_id, &FIRST_BID);
        client(&setup).place_bid(&second_bidder, &listing_id, &750i128);
    }

    #[test]
    fn reserve_not_met_refunds_bidder_and_expires() {
        let setup = setup();
        let seller = Address::generate(&setup.env);
        let bidder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&bidder, &FIRST_BID);
        let listing_id = create_auction(&setup, &seller, PRICE, 0i128);

        client(&setup).place_bid(&bidder, &listing_id, &FIRST_BID);
        setup.env.ledger().set_sequence_number(DEADLINE_LEDGER + 1);
        client(&setup).settle_auction(&listing_id);

        assert!(client(&setup).get_listing(&listing_id).status == ListingStatus::Expired);
        assert!(token_client(&setup.env, &setup.token).balance(&bidder) == FIRST_BID);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.contract_id) == 0);
        assert!(token_client(&setup.env, &setup.token).balance(&seller) == 0);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.admin) == 0);
    }

    #[test]
    fn reserve_and_increment_auction_settles_normally() {
        let setup = setup();
        let seller = Address::generate(&setup.env);
        let first_bidder = Address::generate(&setup.env);
        let second_bidder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&first_bidder, &FIRST_BID);
        asset_client(&setup.env, &setup.token).mint(&second_bidder, &SECOND_BID);
        let listing_id = create_auction(&setup, &seller, 800i128, 100i128);

        client(&setup).place_bid(&first_bidder, &listing_id, &FIRST_BID);
        client(&setup).place_bid(&second_bidder, &listing_id, &SECOND_BID);
        setup.env.ledger().set_sequence_number(DEADLINE_LEDGER + 1);
        client(&setup).settle_auction(&listing_id);

        let fee = SECOND_BID * FEE_RATE_BPS as i128 / 10_000;
        assert!(client(&setup).get_listing(&listing_id).status == ListingStatus::Sold);
        assert!(token_client(&setup.env, &setup.token).balance(&first_bidder) == FIRST_BID);
        assert!(token_client(&setup.env, &setup.token).balance(&seller) == SECOND_BID - fee);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.admin) == fee);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.contract_id) == 0);
    }

    #[test]
    fn zero_reserve_and_increment_preserves_existing_auction_flow() {
        let setup = setup();
        let seller = Address::generate(&setup.env);
        let bidder = Address::generate(&setup.env);
        asset_client(&setup.env, &setup.token).mint(&bidder, &FIRST_BID);
        let listing_id = create_auction(&setup, &seller, 0i128, 0i128);

        client(&setup).place_bid(&bidder, &listing_id, &FIRST_BID);
        setup.env.ledger().set_sequence_number(DEADLINE_LEDGER + 1);
        client(&setup).settle_auction(&listing_id);

        let fee = FIRST_BID * FEE_RATE_BPS as i128 / 10_000;
        assert!(client(&setup).get_listing(&listing_id).status == ListingStatus::Sold);
        assert!(token_client(&setup.env, &setup.token).balance(&seller) == FIRST_BID - fee);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.admin) == fee);
        assert!(token_client(&setup.env, &setup.token).balance(&setup.contract_id) == 0);
    }
}
