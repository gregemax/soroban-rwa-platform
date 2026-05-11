#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, token, Address, Env};

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    FeeRate, // basis points
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
        env.storage()
            .instance()
            .set(&DataKey::FeeRate, &fee_rate_bps);
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
        assert!(
            listing.listing_type == ListingType::FixedPrice,
            "not fixed price"
        );
        assert!(
            env.ledger().sequence() <= listing.deadline_ledger,
            "listing expired"
        );

        let fee_rate: u32 = env.storage().instance().get(&DataKey::FeeRate).unwrap_or(0);
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
        assert!(amount > listing.highest_bid, "bid too low");

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
            let fee_rate: u32 = env.storage().instance().get(&DataKey::FeeRate).unwrap_or(0);
            let fee = listing.highest_bid * fee_rate as i128 / 10_000;
            let seller_amount = listing.highest_bid - fee;

            let tok = token::Client::new(&env, &listing.token);
            tok.transfer(
                &env.current_contract_address(),
                &listing.seller,
                &seller_amount,
            );
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
        assert!(
            listing.listing_type == ListingType::FixedPrice,
            "not fixed price"
        );
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
