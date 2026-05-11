#![no_std]
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String};

// ── Storage keys ────────────────────────────────────────────────────────────

#[contracttype]
/// Storage keys used by the asset registry contract.
pub enum DataKey {
    /// Contract administrator address.
    Admin,
    /// Registered verifier flag keyed by verifier address.
    Verifier(Address),
    /// Registered asset record keyed by asset id.
    Asset(u64),
    /// Monotonic counter used to assign asset ids.
    AssetCount,
}

// ── Types ────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
/// Supported categories for tokenized real-world assets.
pub enum AssetType {
    /// Real estate properties and land.
    RealEstate,
    /// Gold-backed assets.
    Gold,
    /// Agricultural assets.
    Agriculture,
    /// Generic commodities.
    Commodity,
    /// Any asset type not covered by the predefined categories.
    Other,
}

#[contracttype]
#[derive(Clone, PartialEq)]
/// Verification and lifecycle status for a registered real-world asset.
pub enum AssetStatus {
    /// The asset has been registered and is waiting for verifier review.
    Pending,
    /// A registered verifier has approved the asset.
    Verified,
    /// A registered verifier has rejected the asset.
    Rejected,
    /// The admin has temporarily frozen the asset.
    Frozen,
    /// The owner has permanently retired the asset.
    Retired,
}

#[contracttype]
#[derive(Clone)]
/// Metadata and lifecycle state for a registered real-world asset.
pub struct RwaAsset {
    /// Address that owns and controls the asset record.
    pub owner: Address,
    /// Category of the real-world asset.
    pub asset_type: AssetType,
    /// Human-readable asset name.
    pub name: String,
    /// Human-readable asset description.
    pub description: String,
    /// Physical or jurisdictional asset location.
    pub location: String,
    /// Hash or reference for the associated legal documentation.
    pub legal_doc_hash: String,
    /// Latest appraised value for the asset.
    pub appraised_value: i128,
    /// Currency code used for the appraisal value.
    pub appraisal_currency: String,
    /// Total number of fractional shares represented by the asset.
    pub total_shares: i128,
    /// Current verification and lifecycle status.
    pub status: AssetStatus,
    /// Verifier that last reviewed the asset, when available.
    pub verifier: Option<Address>,
    /// Ledger sequence when the asset was registered.
    pub created_ledger: u32,
    /// Ledger sequence when the asset was verified, or zero if unverified.
    pub verified_ledger: u32,
}

// ── Contract ─────────────────────────────────────────────────────────────────

#[contract]
/// Contract for registering, verifying, freezing, and retiring RWA records.
pub struct AssetRegistry;

#[contractimpl]
impl AssetRegistry {
    /// Initialize the registry administrator and asset counter.
    ///
    /// # Panics
    ///
    /// Panics if the contract has already been initialized.
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::AssetCount, &0u64);
    }

    /// Register an address as an approved verifier.
    ///
    /// # Panics
    ///
    /// Panics if the stored admin does not authorize the call.
    pub fn register_verifier(env: Env, verifier: Address) {
        Self::require_admin(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Verifier(verifier.clone()), &true);
        env.events()
            .publish((symbol_short!("ver_add"),), (verifier,));
    }

    /// Remove an address from the verifier allowlist.
    ///
    /// # Panics
    ///
    /// Panics if the stored admin does not authorize the call.
    pub fn remove_verifier(env: Env, verifier: Address) {
        Self::require_admin(&env);
        env.storage()
            .persistent()
            .remove(&DataKey::Verifier(verifier.clone()));
        env.events()
            .publish((symbol_short!("ver_rm"),), (verifier,));
    }

    /// Register a new asset in pending status and return its assigned id.
    ///
    /// # Panics
    ///
    /// Panics if the owner does not authorize the call.
    pub fn register_asset(
        env: Env,
        owner: Address,
        asset_type: AssetType,
        name: String,
        description: String,
        location: String,
        legal_doc_hash: String,
        appraised_value: i128,
        appraisal_currency: String,
        total_shares: i128,
    ) -> u64 {
        owner.require_auth();
        let id: u64 = env
            .storage()
            .instance()
            .get(&DataKey::AssetCount)
            .unwrap_or(0);
        let asset = RwaAsset {
            owner: owner.clone(),
            asset_type,
            name,
            description,
            location,
            legal_doc_hash,
            appraised_value,
            appraisal_currency,
            total_shares,
            status: AssetStatus::Pending,
            verifier: None,
            created_ledger: env.ledger().sequence(),
            verified_ledger: 0,
        };
        env.storage().persistent().set(&DataKey::Asset(id), &asset);
        env.storage()
            .instance()
            .set(&DataKey::AssetCount, &(id + 1));
        env.events()
            .publish((symbol_short!("reg_asset"),), (id, owner));
        id
    }

    /// Mark a pending asset as verified by an approved verifier.
    ///
    /// # Panics
    ///
    /// Panics if the verifier does not authorize the call, is not registered,
    /// the asset does not exist, or the asset is not pending.
    pub fn verify_asset(env: Env, verifier: Address, asset_id: u64) {
        verifier.require_auth();
        Self::require_verifier(&env, &verifier);
        let mut asset = Self::load_asset(&env, asset_id);
        assert!(asset.status == AssetStatus::Pending, "not pending");
        asset.status = AssetStatus::Verified;
        asset.verifier = Some(verifier.clone());
        asset.verified_ledger = env.ledger().sequence();
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events()
            .publish((symbol_short!("verified"),), (asset_id, verifier));
    }

    /// Mark a pending asset as rejected by an approved verifier.
    ///
    /// # Panics
    ///
    /// Panics if the verifier does not authorize the call, is not registered,
    /// the asset does not exist, or the asset is not pending.
    pub fn reject_asset(env: Env, verifier: Address, asset_id: u64) {
        verifier.require_auth();
        Self::require_verifier(&env, &verifier);
        let mut asset = Self::load_asset(&env, asset_id);
        assert!(asset.status == AssetStatus::Pending, "not pending");
        asset.status = AssetStatus::Rejected;
        asset.verifier = Some(verifier.clone());
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events()
            .publish((symbol_short!("rejected"),), (asset_id, verifier));
    }

    /// Freeze a verified asset.
    ///
    /// # Panics
    ///
    /// Panics if the admin does not authorize the call, the asset does not
    /// exist, or the asset is not verified.
    pub fn freeze_asset(env: Env, asset_id: u64) {
        Self::require_admin(&env);
        let mut asset = Self::load_asset(&env, asset_id);
        assert!(asset.status == AssetStatus::Verified, "not verified");
        asset.status = AssetStatus::Frozen;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events()
            .publish((symbol_short!("frozen"),), (asset_id,));
    }

    /// Return a frozen asset to verified status.
    ///
    /// # Panics
    ///
    /// Panics if the admin does not authorize the call, the asset does not
    /// exist, or the asset is not frozen.
    pub fn unfreeze_asset(env: Env, asset_id: u64) {
        Self::require_admin(&env);
        let mut asset = Self::load_asset(&env, asset_id);
        assert!(asset.status == AssetStatus::Frozen, "not frozen");
        asset.status = AssetStatus::Verified;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events()
            .publish((symbol_short!("unfrozen"),), (asset_id,));
    }

    /// Update an asset appraisal and return the asset to pending review.
    ///
    /// # Panics
    ///
    /// Panics if the asset does not exist or the asset owner does not authorize
    /// the call.
    pub fn update_appraisal(env: Env, asset_id: u64, new_value: i128, currency: String) {
        let mut asset = Self::load_asset(&env, asset_id);
        asset.owner.require_auth();
        asset.appraised_value = new_value;
        asset.appraisal_currency = currency;
        asset.status = AssetStatus::Pending;
        asset.verifier = None;
        asset.verified_ledger = 0;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events()
            .publish((symbol_short!("appraisal"),), (asset_id, new_value));
    }

    /// Retire a verified or rejected asset.
    ///
    /// # Panics
    ///
    /// Panics if the asset does not exist, the asset owner does not authorize
    /// the call, or the asset is not verified or rejected.
    pub fn retire_asset(env: Env, asset_id: u64) {
        let mut asset = Self::load_asset(&env, asset_id);
        asset.owner.require_auth();
        assert!(
            asset.status == AssetStatus::Verified || asset.status == AssetStatus::Rejected,
            "cannot retire"
        );
        asset.status = AssetStatus::Retired;
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events()
            .publish((symbol_short!("retired"),), (asset_id,));
    }

    /// Return the full asset record for an asset id.
    ///
    /// # Panics
    ///
    /// Panics if the asset does not exist.
    pub fn get_asset(env: Env, asset_id: u64) -> RwaAsset {
        Self::load_asset(&env, asset_id)
    }

    /// Return whether an asset is currently verified.
    ///
    /// # Panics
    ///
    /// Panics if the asset does not exist.
    pub fn is_verified(env: Env, asset_id: u64) -> bool {
        Self::load_asset(&env, asset_id).status == AssetStatus::Verified
    }

    // ── Helpers ──────────────────────────────────────────────────────────────

    fn require_admin(env: &Env) {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("not initialized");
        admin.require_auth();
    }

    fn require_verifier(env: &Env, verifier: &Address) {
        assert!(
            env.storage()
                .persistent()
                .get::<_, bool>(&DataKey::Verifier(verifier.clone()))
                .unwrap_or(false),
            "not a verifier"
        );
    }

    fn load_asset(env: &Env, asset_id: u64) -> RwaAsset {
        env.storage()
            .persistent()
            .get(&DataKey::Asset(asset_id))
            .expect("asset not found")
    }
}
