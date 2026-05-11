#![no_std]
use soroban_sdk::{contract, contractimpl, contracttype, symbol_short, Address, Env, String};

// ── Storage keys ────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier(Address),
    Asset(u64),
    AssetCount,
}

// ── Types ────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum AssetType {
    RealEstate,
    Gold,
    Agriculture,
    Commodity,
    Other,
}

#[contracttype]
#[derive(Clone, PartialEq)]
pub enum AssetStatus {
    Pending,
    Verified,
    Rejected,
    Frozen,
    Retired,
}

#[contracttype]
#[derive(Clone)]
pub struct RwaAsset {
    pub owner: Address,
    pub asset_type: AssetType,
    pub name: String,
    pub description: String,
    pub location: String,
    pub legal_doc_hash: String,
    pub appraised_value: i128,
    pub appraisal_currency: String,
    pub total_shares: i128,
    pub status: AssetStatus,
    pub verifier: Option<Address>,
    pub created_ledger: u32,
    pub verified_ledger: u32,
}

// ── Contract ─────────────────────────────────────────────────────────────────

#[contract]
pub struct AssetRegistry;

#[contractimpl]
impl AssetRegistry {
    pub fn initialize(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::AssetCount, &0u64);
    }

    pub fn register_verifier(env: Env, verifier: Address) {
        Self::require_admin(&env);
        env.storage()
            .persistent()
            .set(&DataKey::Verifier(verifier.clone()), &true);
        env.events()
            .publish((symbol_short!("ver_add"),), (verifier,));
    }

    pub fn remove_verifier(env: Env, verifier: Address) {
        Self::require_admin(&env);
        env.storage()
            .persistent()
            .remove(&DataKey::Verifier(verifier.clone()));
        env.events()
            .publish((symbol_short!("ver_rm"),), (verifier,));
    }

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

    pub fn get_asset(env: Env, asset_id: u64) -> RwaAsset {
        Self::load_asset(&env, asset_id)
    }

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
