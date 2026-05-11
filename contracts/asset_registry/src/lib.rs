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

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::testutils::{Address as _, Events};

    fn setup() -> (Env, Address, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let contract_id = env.register_contract(None, AssetRegistry);
        let client = AssetRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        let verifier = Address::generate(&env);

        client.initialize(&admin);

        (env, contract_id, admin, owner, verifier)
    }

    fn string(env: &Env, value: &str) -> String {
        String::from_str(env, value)
    }

    fn register_sample_asset(client: &AssetRegistryClient, env: &Env, owner: &Address) -> u64 {
        client.register_asset(
            owner,
            &AssetType::RealEstate,
            &string(env, "Office Tower"),
            &string(env, "Class A commercial office building"),
            &string(env, "Seoul"),
            &string(env, "ipfs://legal-doc"),
            &1_500_000,
            &string(env, "USD"),
            &10_000,
        )
    }

    #[test]
    fn initialize_rejects_second_call() {
        let (env, contract_id, admin, _, _) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);

        let result = client.try_initialize(&admin);

        assert!(result.is_err());
    }

    #[test]
    fn register_asset_sets_pending_defaults_and_emits_event() {
        let (env, contract_id, _, owner, _) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);

        let asset_id = register_sample_asset(&client, &env, &owner);
        let asset = client.get_asset(&asset_id);

        assert_eq!(asset_id, 0);
        assert_eq!(asset.owner, owner);
        assert!(asset.asset_type == AssetType::RealEstate);
        assert_eq!(asset.name, string(&env, "Office Tower"));
        assert_eq!(
            asset.description,
            string(&env, "Class A commercial office building")
        );
        assert_eq!(asset.location, string(&env, "Seoul"));
        assert_eq!(asset.legal_doc_hash, string(&env, "ipfs://legal-doc"));
        assert_eq!(asset.appraised_value, 1_500_000);
        assert_eq!(asset.appraisal_currency, string(&env, "USD"));
        assert_eq!(asset.total_shares, 10_000);
        assert!(asset.status == AssetStatus::Pending);
        assert!(asset.verifier.is_none());
        assert_eq!(asset.created_ledger, env.ledger().sequence());
        assert_eq!(asset.verified_ledger, 0);
        assert_eq!(env.events().all().len(), 1);
    }

    #[test]
    fn register_verifier_then_verify_asset() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        let asset_id = register_sample_asset(&client, &env, &owner);

        client.verify_asset(&verifier, &asset_id);

        let asset = client.get_asset(&asset_id);
        assert!(asset.status == AssetStatus::Verified);
        assert_eq!(asset.verifier, Some(verifier));
        assert_eq!(asset.verified_ledger, env.ledger().sequence());
        assert!(client.is_verified(&asset_id));
        assert_eq!(env.events().all().len(), 3);
    }

    #[test]
    fn reject_asset_marks_rejected() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        let asset_id = register_sample_asset(&client, &env, &owner);

        client.reject_asset(&verifier, &asset_id);

        let asset = client.get_asset(&asset_id);
        assert!(asset.status == AssetStatus::Rejected);
        assert_eq!(asset.verifier, Some(verifier));
        assert!(!client.is_verified(&asset_id));
    }

    #[test]
    fn remove_verifier_blocks_future_verification() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        client.remove_verifier(&verifier);
        let asset_id = register_sample_asset(&client, &env, &owner);

        let result = client.try_verify_asset(&verifier, &asset_id);

        assert!(result.is_err());
    }

    #[test]
    fn verifier_cannot_verify_non_pending_asset_twice() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        let asset_id = register_sample_asset(&client, &env, &owner);
        client.verify_asset(&verifier, &asset_id);

        let result = client.try_verify_asset(&verifier, &asset_id);

        assert!(result.is_err());
    }

    #[test]
    fn freeze_and_unfreeze_verified_asset() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        let asset_id = register_sample_asset(&client, &env, &owner);
        client.verify_asset(&verifier, &asset_id);

        client.freeze_asset(&asset_id);
        let frozen = client.get_asset(&asset_id);
        assert!(frozen.status == AssetStatus::Frozen);
        assert!(!client.is_verified(&asset_id));

        client.unfreeze_asset(&asset_id);
        let unfrozen = client.get_asset(&asset_id);
        assert!(unfrozen.status == AssetStatus::Verified);
        assert!(client.is_verified(&asset_id));
    }

    #[test]
    fn freeze_rejects_pending_asset() {
        let (env, contract_id, _, owner, _) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let asset_id = register_sample_asset(&client, &env, &owner);

        let result = client.try_freeze_asset(&asset_id);

        assert!(result.is_err());
    }

    #[test]
    fn update_appraisal_resets_verification_state() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        let asset_id = register_sample_asset(&client, &env, &owner);
        client.verify_asset(&verifier, &asset_id);

        client.update_appraisal(&asset_id, &2_000_000, &string(&env, "EUR"));

        let asset = client.get_asset(&asset_id);
        assert_eq!(asset.appraised_value, 2_000_000);
        assert_eq!(asset.appraisal_currency, string(&env, "EUR"));
        assert!(asset.status == AssetStatus::Pending);
        assert!(asset.verifier.is_none());
        assert_eq!(asset.verified_ledger, 0);
        assert!(!client.is_verified(&asset_id));
    }

    #[test]
    fn retire_asset_accepts_verified_and_rejected_assets() {
        let (env, contract_id, _, owner, verifier) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        client.register_verifier(&verifier);
        let verified_id = register_sample_asset(&client, &env, &owner);
        let rejected_id = register_sample_asset(&client, &env, &owner);
        client.verify_asset(&verifier, &verified_id);
        client.reject_asset(&verifier, &rejected_id);

        client.retire_asset(&verified_id);
        client.retire_asset(&rejected_id);

        assert!(client.get_asset(&verified_id).status == AssetStatus::Retired);
        assert!(client.get_asset(&rejected_id).status == AssetStatus::Retired);
    }

    #[test]
    fn retire_rejects_pending_asset() {
        let (env, contract_id, _, owner, _) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let asset_id = register_sample_asset(&client, &env, &owner);

        let result = client.try_retire_asset(&asset_id);

        assert!(result.is_err());
    }

    #[test]
    fn register_verifier_requires_admin_auth() {
        let env = Env::default();
        let contract_id = env.register_contract(None, AssetRegistry);
        let client = AssetRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let verifier = Address::generate(&env);
        client.initialize(&admin);

        let result = client.try_register_verifier(&verifier);

        assert!(result.is_err());
    }

    #[test]
    fn register_asset_requires_owner_auth() {
        let env = Env::default();
        let contract_id = env.register_contract(None, AssetRegistry);
        let client = AssetRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        let owner = Address::generate(&env);
        client.initialize(&admin);

        let result = client.try_register_asset(
            &owner,
            &AssetType::Gold,
            &string(&env, "Gold Reserve"),
            &string(&env, "Allocated bullion"),
            &string(&env, "Zurich"),
            &string(&env, "ipfs://gold-doc"),
            &500_000,
            &string(&env, "USD"),
            &1_000,
        );

        assert!(result.is_err());
    }

    #[test]
    fn get_asset_rejects_unknown_id() {
        let (env, contract_id, _, _, _) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);

        let result = client.try_get_asset(&99);

        assert!(result.is_err());
    }
}
