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
            .publish((Symbol::new(&env, "registered"),), (id, owner));
        id
    }

    pub fn transfer_ownership(env: Env, asset_id: u64, new_owner: Address) {
        let mut asset = Self::load_asset(&env, asset_id);
        asset.owner.require_auth();
        assert!(
            asset.status == AssetStatus::Verified || asset.status == AssetStatus::Pending,
            "cannot transfer"
        );

        let old_owner = asset.owner.clone();
        asset.owner = new_owner.clone();
        env.storage()
            .persistent()
            .set(&DataKey::Asset(asset_id), &asset);
        env.events().publish(
            (
                symbol_short!("asset"),
                Symbol::new(&env, "ownership_transferred"),
            ),
            (asset_id, old_owner, new_owner),
        );
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
        testutils::{Address as _, MockAuth, MockAuthInvoke},
        IntoVal,
    };

    fn setup() -> (Env, Address, Address) {
        let env = Env::default();
        let contract_id = env.register_contract(None, AssetRegistry);
        let client = AssetRegistryClient::new(&env, &contract_id);
        let admin = Address::generate(&env);
        client.initialize(&admin);

        (env, contract_id, admin)
    }

    fn register_asset(env: &Env, client: &AssetRegistryClient, owner: &Address) -> u64 {
        client.register_asset(
            owner,
            &AssetType::RealEstate,
            &String::from_str(env, "Warehouse"),
            &String::from_str(env, "Tokenized warehouse"),
            &String::from_str(env, "New York"),
            &String::from_str(env, "legal-hash"),
            &1_000_000i128,
            &String::from_str(env, "USD"),
            &1_000i128,
        )
    }

    fn register_asset_with_owner_auth(
        env: &Env,
        contract_id: &Address,
        client: &AssetRegistryClient,
        owner: &Address,
    ) -> u64 {
        let name = String::from_str(env, "Warehouse");
        let description = String::from_str(env, "Tokenized warehouse");
        let location = String::from_str(env, "New York");
        let legal_doc_hash = String::from_str(env, "legal-hash");
        let currency = String::from_str(env, "USD");

        client
            .mock_auths(&[MockAuth {
                address: owner,
                invoke: &MockAuthInvoke {
                    contract: contract_id,
                    fn_name: "register_asset",
                    args: (
                        owner,
                        AssetType::RealEstate,
                        name.clone(),
                        description.clone(),
                        location.clone(),
                        legal_doc_hash.clone(),
                        1_000_000i128,
                        currency.clone(),
                        1_000i128,
                    )
                        .into_val(env),
                    sub_invokes: &[],
                },
            }])
            .register_asset(
                owner,
                &AssetType::RealEstate,
                &name,
                &description,
                &location,
                &legal_doc_hash,
                &1_000_000i128,
                &currency,
                &1_000i128,
            )
    }

    fn register_verifier(
        env: &Env,
        contract_id: &Address,
        client: &AssetRegistryClient,
        admin: &Address,
        verifier: &Address,
    ) {
        client
            .mock_auths(&[MockAuth {
                address: admin,
                invoke: &MockAuthInvoke {
                    contract: contract_id,
                    fn_name: "register_verifier",
                    args: (verifier,).into_val(env),
                    sub_invokes: &[],
                },
            }])
            .register_verifier(verifier);
    }

    fn verify_asset(
        env: &Env,
        contract_id: &Address,
        client: &AssetRegistryClient,
        verifier: &Address,
        asset_id: u64,
    ) {
        client
            .mock_auths(&[MockAuth {
                address: verifier,
                invoke: &MockAuthInvoke {
                    contract: contract_id,
                    fn_name: "verify_asset",
                    args: (verifier, asset_id).into_val(env),
                    sub_invokes: &[],
                },
            }])
            .verify_asset(verifier, &asset_id);
    }

    fn transfer_ownership(
        env: &Env,
        contract_id: &Address,
        client: &AssetRegistryClient,
        owner: &Address,
        asset_id: u64,
        new_owner: &Address,
    ) {
        client
            .mock_auths(&[MockAuth {
                address: owner,
                invoke: &MockAuthInvoke {
                    contract: contract_id,
                    fn_name: "transfer_ownership",
                    args: (asset_id, new_owner).into_val(env),
                    sub_invokes: &[],
                },
            }])
            .transfer_ownership(&asset_id, new_owner);
    }

    #[test]
    fn transfer_ownership_updates_pending_asset_owner() {
        let (env, contract_id, _admin) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let asset_id = register_asset_with_owner_auth(&env, &contract_id, &client, &owner);

        transfer_ownership(&env, &contract_id, &client, &owner, asset_id, &new_owner);

        let asset = client.get_asset(&asset_id);
        assert!(asset.owner == new_owner);
        assert!(asset.status == AssetStatus::Pending);
    }

    #[test]
    fn transfer_ownership_updates_verified_asset_owner() {
        let (env, contract_id, admin) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let verifier = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let asset_id = register_asset_with_owner_auth(&env, &contract_id, &client, &owner);

        register_verifier(&env, &contract_id, &client, &admin, &verifier);
        verify_asset(&env, &contract_id, &client, &verifier, asset_id);
        transfer_ownership(&env, &contract_id, &client, &owner, asset_id, &new_owner);

        let asset = client.get_asset(&asset_id);
        assert!(asset.owner == new_owner);
        assert!(asset.status == AssetStatus::Verified);
    }

    #[test]
    #[should_panic]
    fn transfer_ownership_requires_current_owner_auth() {
        let (env, contract_id, _admin) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let wrong_owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let asset_id = register_asset_with_owner_auth(&env, &contract_id, &client, &owner);

        transfer_ownership(
            &env,
            &contract_id,
            &client,
            &wrong_owner,
            asset_id,
            &new_owner,
        );
    }

    #[test]
    #[should_panic]
    fn transfer_ownership_rejects_invalid_status() {
        let (env, contract_id, _admin) = setup();
        env.mock_all_auths();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let verifier = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let asset_id = register_asset(&env, &client, &owner);

        client.register_verifier(&verifier);
        client.reject_asset(&verifier, &asset_id);
        client.transfer_ownership(&asset_id, &new_owner);
    }

    #[test]
    fn new_owner_can_use_owner_gated_functions_after_transfer() {
        let (env, contract_id, _admin) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let asset_id = register_asset_with_owner_auth(&env, &contract_id, &client, &owner);

        transfer_ownership(&env, &contract_id, &client, &owner, asset_id, &new_owner);
        client
            .mock_auths(&[MockAuth {
                address: &new_owner,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "update_appraisal",
                    args: (asset_id, 2_500_000i128, String::from_str(&env, "USD")).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .update_appraisal(&asset_id, &2_500_000i128, &String::from_str(&env, "USD"));

        let asset = client.get_asset(&asset_id);
        assert!(asset.owner == new_owner);
        assert!(asset.appraised_value == 2_500_000i128);
    }

    #[test]
    #[should_panic]
    fn old_owner_auth_cannot_use_owner_gated_functions_after_transfer() {
        let (env, contract_id, _admin) = setup();
        let client = AssetRegistryClient::new(&env, &contract_id);
        let owner = Address::generate(&env);
        let new_owner = Address::generate(&env);
        let asset_id = register_asset_with_owner_auth(&env, &contract_id, &client, &owner);

        transfer_ownership(&env, &contract_id, &client, &owner, asset_id, &new_owner);
        client
            .mock_auths(&[MockAuth {
                address: &owner,
                invoke: &MockAuthInvoke {
                    contract: &contract_id,
                    fn_name: "update_appraisal",
                    args: (asset_id, 2_500_000i128, String::from_str(&env, "USD")).into_val(&env),
                    sub_invokes: &[],
                },
            }])
            .update_appraisal(&asset_id, &2_500_000i128, &String::from_str(&env, "USD"));
    }
}
