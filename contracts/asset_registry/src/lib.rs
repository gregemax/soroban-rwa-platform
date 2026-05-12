#![no_std]
#![allow(clippy::too_many_arguments)]
use soroban_sdk::{
    contract, contractimpl, contracttype, symbol_short, Address, Env, String, Symbol,
};

// ── Storage keys ────────────────────────────────────────────────────────────

#[contracttype]
pub enum DataKey {
    Admin,
    Verifier(Address),
    KycEnabled,
    KycApproved(Address),
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
        env.storage().instance().set(&DataKey::KycEnabled, &false);
        env.storage().instance().set(&DataKey::AssetCount, &0u64);
    }

    pub fn set_kyc_enabled(env: Env, enabled: bool) {
        Self::require_admin(&env);
        env.storage().instance().set(&DataKey::KycEnabled, &enabled);
        env.events()
            .publish((Symbol::new(&env, "kyc_enabled"),), (enabled,));
    }

    pub fn approve_kyc(env: Env, user: Address) {
        Self::require_admin(&env);
        env.storage()
            .persistent()
            .set(&DataKey::KycApproved(user.clone()), &true);
        env.events()
            .publish((Symbol::new(&env, "kyc_approved"),), (user,));
    }

    pub fn revoke_kyc(env: Env, user: Address) {
        Self::require_admin(&env);
        env.storage()
            .persistent()
            .set(&DataKey::KycApproved(user.clone()), &false);
        env.events()
            .publish((Symbol::new(&env, "kyc_revoked"),), (user,));
    }

    pub fn is_kyc_approved(env: Env, user: Address) -> bool {
        Self::kyc_approved(&env, &user)
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
        if Self::kyc_enabled(&env) {
            assert!(Self::kyc_approved(&env, &owner), "kyc required");
        }
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
            .publish((Symbol::new(&env, "registered"),), (id, owner));
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

    pub fn update_appraisal(
        env: Env,
        asset_id: u64,
        new_value: i128,
        currency: String,
    ) {
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
            asset.status == AssetStatus::Verified
                || asset.status == AssetStatus::Rejected,
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

    fn kyc_enabled(env: &Env) -> bool {
        env.storage()
            .instance()
            .get::<_, bool>(&DataKey::KycEnabled)
            .unwrap_or(false)
    }

    fn kyc_approved(env: &Env, user: &Address) -> bool {
        env.storage()
            .persistent()
            .get::<_, bool>(&DataKey::KycApproved(user.clone()))
            .unwrap_or(false)
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
    use soroban_sdk::{
        testutils::{Address as _, Events, Ledger, MockAuth, MockAuthInvoke},
        IntoVal, Symbol, TryIntoVal,
    };

    const APPRAISED_VALUE: i128 = 500_000;
    const TOTAL_SHARES: i128 = 10_000;

    struct Setup {
        env: Env,
        contract_id: Address,
    }

    fn client(setup: &Setup) -> AssetRegistryClient<'_> {
        AssetRegistryClient::new(&setup.env, &setup.contract_id)
    }

    fn setup() -> Setup {
        let env = Env::default();
        env.mock_all_auths();
        env.ledger().set_sequence_number(10);
        let contract_id = env.register_contract(None, AssetRegistry);
        let client = AssetRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);

        Setup { env, contract_id }
    }

    fn setup_without_mock_all_auths() -> Setup {
        let env = Env::default();
        env.ledger().set_sequence_number(10);
        let contract_id = env.register_contract(None, AssetRegistry);
        let client = AssetRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);

        Setup { env, contract_id }
    }

    fn asset_string(env: &Env, value: &str) -> String {
        String::from_str(env, value)
    }

    fn register_asset(setup: &Setup, owner: &Address) -> u64 {
        client(setup).register_asset(
            owner,
            &AssetType::RealEstate,
            &asset_string(&setup.env, "warehouse"),
            &asset_string(&setup.env, "leased logistics warehouse"),
            &asset_string(&setup.env, "new york"),
            &asset_string(&setup.env, "ipfs://legal-doc"),
            &APPRAISED_VALUE,
            &asset_string(&setup.env, "USD"),
            &TOTAL_SHARES,
        )
    }

    fn set_kyc_enabled_with_auth(setup: &Setup, signer: &Address, enabled: bool) {
        client(setup)
            .mock_auths(&[MockAuth {
                address: signer,
                invoke: &MockAuthInvoke {
                    contract: &setup.contract_id,
                    fn_name: "set_kyc_enabled",
                    args: (enabled,).into_val(&setup.env),
                    sub_invokes: &[],
                },
            }])
            .set_kyc_enabled(&enabled);
    }

    #[test]
    fn initialize_disables_kyc_by_default() {
        let setup = setup();
        let stored: bool = setup.env.as_contract(&setup.contract_id, || {
            setup
                .env
                .storage()
                .instance()
                .get(&DataKey::KycEnabled)
                .unwrap()
        });

        assert!(!stored);
    }

    #[test]
    fn set_kyc_enabled_stores_flag_and_emits_event() {
        let setup = setup();

        client(&setup).set_kyc_enabled(&true);

        let stored: bool = setup.env.as_contract(&setup.contract_id, || {
            setup
                .env
                .storage()
                .instance()
                .get(&DataKey::KycEnabled)
                .unwrap()
        });
        let events = setup.env.events().all();
        let (_contract, topics, data) = events.get(events.len() - 1).unwrap();
        let topic: Symbol = topics.get(0).unwrap().try_into_val(&setup.env).unwrap();
        let event_data: (bool,) = data.try_into_val(&setup.env).unwrap();

        assert!(stored);
        assert!(topic == Symbol::new(&setup.env, "kyc_enabled"));
        assert!(event_data == (true,));
    }

    #[test]
    fn approve_and_revoke_kyc_update_flag_and_emit_events() {
        let setup = setup();
        let user = Address::generate(&setup.env);

        client(&setup).approve_kyc(&user);

        assert!(client(&setup).is_kyc_approved(&user));
        let events = setup.env.events().all();
        let (_contract, topics, data) = events.get(events.len() - 1).unwrap();
        let topic: Symbol = topics.get(0).unwrap().try_into_val(&setup.env).unwrap();
        let event_data: (Address,) = data.try_into_val(&setup.env).unwrap();
        assert!(topic == Symbol::new(&setup.env, "kyc_approved"));
        assert!(event_data == (user.clone(),));

        client(&setup).revoke_kyc(&user);

        assert!(!client(&setup).is_kyc_approved(&user));
        let events = setup.env.events().all();
        let (_contract, topics, data) = events.get(events.len() - 1).unwrap();
        let topic: Symbol = topics.get(0).unwrap().try_into_val(&setup.env).unwrap();
        let event_data: (Address,) = data.try_into_val(&setup.env).unwrap();
        assert!(topic == Symbol::new(&setup.env, "kyc_revoked"));
        assert!(event_data == (user,));
    }

    #[test]
    fn register_asset_works_when_kyc_disabled() {
        let setup = setup();
        let owner = Address::generate(&setup.env);

        let asset_id = register_asset(&setup, &owner);

        let asset = client(&setup).get_asset(&asset_id);
        assert!(asset.owner == owner);
        assert!(asset.status == AssetStatus::Pending);
    }

    #[test]
    fn register_asset_allows_approved_owner_when_kyc_enabled() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        client(&setup).set_kyc_enabled(&true);
        client(&setup).approve_kyc(&owner);

        let asset_id = register_asset(&setup, &owner);

        assert!(client(&setup).get_asset(&asset_id).owner == owner);
    }

    #[test]
    #[should_panic]
    fn register_asset_rejects_unapproved_owner_when_kyc_enabled() {
        let setup = setup();
        let owner = Address::generate(&setup.env);
        client(&setup).set_kyc_enabled(&true);

        register_asset(&setup, &owner);
    }

    #[test]
    #[should_panic]
    fn set_kyc_enabled_requires_admin_auth() {
        let setup = setup_without_mock_all_auths();
        let other = Address::generate(&setup.env);

        set_kyc_enabled_with_auth(&setup, &other, true);
    }
}
