#![no_std]
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, token, Address, Env};

// ── Storage keys ─────────────────────────────────────────────────────────────

#[contracttype]
/// Storage keys used by the marketplace contract.
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Platform fee rate in basis points.
    FeeRate, // basis points
    /// Marketplace listing keyed by listing id.
    Listing(u64),
    /// Monotonic counter used to assign listing ids.
    ListingCount,
    /// Historical offer keyed by listing id and offer index.
    Offer(u64, u64), // (listing_id, offer_index)
    /// Offer count for a listing.
    OfferCount(u64), // listing_id -> offer count
}

// ── Types ─────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
/// Supported marketplace sale formats.
pub enum ListingType {
    /// Listing can be purchased immediately at a fixed price.
    FixedPrice,
    /// Listing accepts bids until its deadline.
    Auction,
}

#[contracttype]
#[derive(Clone, PartialEq)]
/// Lifecycle state for a marketplace listing.
pub enum ListingStatus {
    /// Listing is open for purchase or bidding.
    Active,
    /// Listing completed successfully.
    Sold,
    /// Listing expired without a sale.
    Expired,
    /// Seller cancelled the listing.
    Cancelled,
}

#[contracttype]
#[derive(Clone)]
/// Marketplace listing for fractional shares.
pub struct Listing {
    /// Address that created and controls the listing.
    pub seller: Address,
    /// Asset whose shares are listed.
    pub asset_id: u64,
    /// Number of shares being sold.
    pub shares: i128,
    /// Token used for payment.
    pub token: Address,
    /// Fixed price or opening auction price.
    pub price: i128,
    /// Current highest auction bid.
    pub highest_bid: i128,
    /// Current highest bidder for an auction listing.
    pub highest_bidder: Option<Address>,
    /// Sale format for the listing.
    pub listing_type: ListingType,
    /// Last ledger sequence where the listing is active.
    pub deadline_ledger: u32,
    /// Current listing lifecycle state.
    pub status: ListingStatus,
}

#[contracttype]
#[derive(Clone)]
/// Recorded auction offer for a listing.
pub struct Offer {
    /// Listing receiving the offer.
    pub listing_id: u64,
    /// Address that placed the bid.
    pub bidder: Address,
    /// Bid amount locked for the offer.
    pub amount: i128,
    /// Whether the offer is still active.
    pub active: bool,
    /// Ledger sequence when the offer was placed.
    pub ledger: u32,
}

// ── Contract ──────────────────────────────────────────────────────────────────

#[contract]
/// Contract for fixed-price and auction sales of fractional shares.
pub struct Marketplace;

#[contractimpl]
impl Marketplace {
    /// Initialize the marketplace with an admin and fee rate.
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
        env.storage().instance().set(&DataKey::ListingCount, &0u64);
    }

    /// Create a fixed-price or auction listing.
    ///
    /// # Panics
    ///
    /// Panics if the seller does not authorize the call.
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
    ///
    /// # Panics
    ///
    /// Panics if the buyer does not authorize the call, the listing does not
    /// exist, is not active, is not fixed-price, has expired, or payment fails.
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
    ///
    /// # Panics
    ///
    /// Panics if the bidder does not authorize the call, the listing does not
    /// exist, is not active, is not an auction, has ended, the bid is too low,
    /// or a token transfer fails.
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
    ///
    /// # Panics
    ///
    /// Panics if the listing does not exist, is not active, is not an auction,
    /// has not ended, or a token transfer fails.
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
    ///
    /// # Panics
    ///
    /// Panics if the listing does not exist, the seller does not authorize the
    /// call, the listing is not active, or a token refund fails.
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
    ///
    /// # Panics
    ///
    /// Panics if the listing does not exist, is not active, is not fixed-price,
    /// or has not expired.
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

    /// Return a listing by id.
    ///
    /// # Panics
    ///
    /// Panics if the listing does not exist.
    pub fn get_listing(env: Env, listing_id: u64) -> Listing {
        Self::load_listing(&env, listing_id)
    }

    /// Return a recorded offer for a listing.
    ///
    /// # Panics
    ///
    /// Panics if the offer does not exist.
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
