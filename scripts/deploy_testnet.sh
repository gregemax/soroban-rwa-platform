#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
NETWORK="${NETWORK:-testnet}"
IDENTITY="${STELLAR_IDENTITY:-rwa-testnet-deployer}"
USDC_ASSET="${USDC_ASSET:-USDC:GCYEIQEWOCTTSA72VPZ6LYIZIK4W4KNGJR72UADIXUXG45VDFRVCQTYE}"
TOKEN_CONTRACT_ID="${TOKEN_CONTRACT_ID:-}"
ASSET_REGISTRY_ALIAS="${ASSET_REGISTRY_ALIAS:-rwa_asset_registry}"
FRACTIONAL_ALIAS="${FRACTIONAL_ALIAS:-rwa_fractional}"
MARKETPLACE_ALIAS="${MARKETPLACE_ALIAS:-rwa_marketplace}"
TOKEN_ALIAS="${TOKEN_ALIAS:-rwa_usdc}"
WASM_TARGET="${WASM_TARGET:-wasm32v1-none}"
PRICE_PER_SHARE="${PRICE_PER_SHARE:-100000000}"
LISTING_SHARE_COUNT="${LISTING_SHARE_COUNT:-100}"
TOTAL_SHARES="${TOTAL_SHARES:-1000}"
MIN_PURCHASE="${MIN_PURCHASE:-1}"
FEE_RATE_BPS="${FEE_RATE_BPS:-100}"
LISTING_DEADLINE_LEDGER="${LISTING_DEADLINE_LEDGER:-429496729}"

ASSET_REGISTRY_WASM="$ROOT_DIR/target/$WASM_TARGET/release/asset_registry.wasm"
FRACTIONAL_WASM="$ROOT_DIR/target/$WASM_TARGET/release/fractional.wasm"
MARKETPLACE_WASM="$ROOT_DIR/target/$WASM_TARGET/release/marketplace.wasm"

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "Missing required command: $1" >&2
    exit 1
  fi
}

last_non_empty_line() {
  /usr/bin/awk 'NF { line = $0 } END { print line }'
}

run() {
  echo "+ $*" >&2
  "$@"
}

deploy_wasm() {
  local wasm_path="$1"
  local alias="$2"
  local contract_id

  if [[ ! -f "$wasm_path" ]]; then
    echo "Expected WASM artifact not found: $wasm_path" >&2
    exit 1
  fi

  contract_id="$(
    stellar contract deploy \
      --wasm "$wasm_path" \
      --source-account "$IDENTITY" \
      --network "$NETWORK" \
      --alias "$alias" \
      | last_non_empty_line
  )"

  if [[ -z "$contract_id" ]]; then
    echo "Deploy for $alias did not return a contract id" >&2
    exit 1
  fi

  printf '%s\n' "$contract_id"
}

invoke() {
  local contract_id="$1"
  shift

  run stellar contract invoke \
    --id "$contract_id" \
    --source-account "$IDENTITY" \
    --network "$NETWORK" \
    --send=yes \
    -- "$@"
}

need_cmd cargo
need_cmd stellar

cd "$ROOT_DIR"

echo "Building release WASM artifacts..."
run cargo build --target "$WASM_TARGET" --release --workspace

if stellar keys address "$IDENTITY" >/dev/null 2>&1; then
  echo "Using existing Stellar identity: $IDENTITY"
else
  echo "Generating and funding Stellar testnet identity: $IDENTITY"
  run stellar keys generate "$IDENTITY" --network "$NETWORK" --fund
fi

DEPLOYER_ADDRESS="$(stellar keys address "$IDENTITY")"
echo "Deployer address: $DEPLOYER_ADDRESS"

if [[ -z "$TOKEN_CONTRACT_ID" ]]; then
  echo "Resolving Stellar Asset Contract for $USDC_ASSET..."
  ASSET_DEPLOY_OUTPUT="$(mktemp)"
  if stellar contract asset deploy \
    --asset "$USDC_ASSET" \
    --source-account "$IDENTITY" \
    --network "$NETWORK" \
    --alias "$TOKEN_ALIAS" >"$ASSET_DEPLOY_OUTPUT" 2>&1; then
    TOKEN_CONTRACT_ID="$(last_non_empty_line <"$ASSET_DEPLOY_OUTPUT")"
  else
    /bin/cat "$ASSET_DEPLOY_OUTPUT" >&2
    if /usr/bin/grep -q "ExistingValue\\|contract already exists" "$ASSET_DEPLOY_OUTPUT"; then
      echo "Asset contract already exists; deriving deterministic contract ID."
      TOKEN_CONTRACT_ID="$(
        stellar contract id asset \
          --asset "$USDC_ASSET" \
          --network "$NETWORK" \
          | last_non_empty_line
      )"
    else
      /bin/rm -f "$ASSET_DEPLOY_OUTPUT"
      exit 1
    fi
  fi
  /bin/rm -f "$ASSET_DEPLOY_OUTPUT"
fi
echo "Token contract ID: $TOKEN_CONTRACT_ID"

echo "Deploying RWA contracts..."
ASSET_REGISTRY_ID="$(deploy_wasm "$ASSET_REGISTRY_WASM" "$ASSET_REGISTRY_ALIAS")"
FRACTIONAL_ID="$(deploy_wasm "$FRACTIONAL_WASM" "$FRACTIONAL_ALIAS")"
MARKETPLACE_ID="$(deploy_wasm "$MARKETPLACE_WASM" "$MARKETPLACE_ALIAS")"

echo "Initializing contracts..."
invoke "$ASSET_REGISTRY_ID" initialize --admin "$DEPLOYER_ADDRESS"
invoke "$ASSET_REGISTRY_ID" register_verifier --verifier "$DEPLOYER_ADDRESS"
invoke "$FRACTIONAL_ID" initialize --admin "$DEPLOYER_ADDRESS" --fee_rate_bps "$FEE_RATE_BPS"
invoke "$MARKETPLACE_ID" initialize --admin "$DEPLOYER_ADDRESS" --fee_rate_bps "$FEE_RATE_BPS"

echo "Registering and verifying sample real estate asset..."
invoke "$ASSET_REGISTRY_ID" register_asset \
  --owner "$DEPLOYER_ADDRESS" \
  --asset_type RealEstate \
  --name "Demo Apartment Building" \
  --description "End-to-end testnet RWA registered by scripts/deploy_testnet.sh" \
  --location "New York, US" \
  --legal_doc_hash "sha256:demo-rwa-legal-document" \
  --appraised_value 10000000000 \
  --appraisal_currency "USDC" \
  --total_shares "$TOTAL_SHARES"
invoke "$ASSET_REGISTRY_ID" verify_asset --verifier "$DEPLOYER_ADDRESS" --asset_id 0

echo "Creating sample fractional offering..."
invoke "$FRACTIONAL_ID" create_offering \
  --owner "$DEPLOYER_ADDRESS" \
  --asset_id 0 \
  --token "$TOKEN_CONTRACT_ID" \
  --total_shares "$TOTAL_SHARES" \
  --price_per_share "$PRICE_PER_SHARE" \
  --min_purchase "$MIN_PURCHASE"

LISTING_PRICE="$((PRICE_PER_SHARE * LISTING_SHARE_COUNT))"
echo "Creating sample fixed-price marketplace listing..."
invoke "$MARKETPLACE_ID" create_listing \
  --seller "$DEPLOYER_ADDRESS" \
  --asset_id 0 \
  --shares "$LISTING_SHARE_COUNT" \
  --token "$TOKEN_CONTRACT_ID" \
  --price "$LISTING_PRICE" \
  --listing_type FixedPrice \
  --deadline_ledger "$LISTING_DEADLINE_LEDGER"

cat <<SUMMARY

Deployment complete.

Network:              $NETWORK
Identity:             $IDENTITY
Deployer:             $DEPLOYER_ADDRESS
Token contract:       $TOKEN_CONTRACT_ID
Asset Registry:       $ASSET_REGISTRY_ID
Fractional:           $FRACTIONAL_ID
Marketplace:          $MARKETPLACE_ID
Sample asset id:      0
Sample offering id:   0
Sample listing id:    0
Listing share count:  $LISTING_SHARE_COUNT
Listing price units:  $LISTING_PRICE

Aliases written by Stellar CLI:
- $TOKEN_ALIAS (when the asset contract was deployed by this script)
- $ASSET_REGISTRY_ALIAS
- $FRACTIONAL_ALIAS
- $MARKETPLACE_ALIAS
SUMMARY
