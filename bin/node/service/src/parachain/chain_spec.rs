///////////////////////////////////////////////////////////////////////////////
//
//  Copyright 2018-2021 Robonomics Network <research@robonomics.network>
//
//  Licensed under the Apache License, Version 2.0 (the "License");
//  you may not use this file except in compliance with the License.
//  You may obtain a copy of the License at
//
//      http://www.apache.org/licenses/LICENSE-2.0
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
//
///////////////////////////////////////////////////////////////////////////////
//! Chain specification and utils.

use alpha_runtime::{
    wasm_binary_unwrap, BalancesConfig, GenesisConfig, ParachainInfoConfig, StakingConfig,
    SudoConfig, SystemConfig,
};
use cumulus_primitives_core::ParaId;
use robonomics_primitives::{AccountId, Balance};
use sc_chain_spec::ChainSpecExtension;
use sc_service::ChainType;
use serde::{Deserialize, Serialize};
use sp_core::sr25519;

use crate::chain_spec::get_account_id_from_seed;

/// Earth parachain ID
const EARTH_ID: u32 = 1000;
/// Mars parachain ID
const MARS_ID: u32 = 2000;
/// Venus parachain ID
const VENUS_ID: u32 = 3000;
/// Uranus parachain ID
const URANUS_ID: u32 = 4000;
/// Kusama parachain ID
const KUSAMA_ID: u32 = 2077;

/// Node `ChainSpec` extensions.
///
/// Additional parameters for some Substrate core modules,
/// customizable from the chain spec.
#[derive(Default, Clone, Serialize, Deserialize, ChainSpecExtension)]
#[serde(rename_all = "camelCase")]
pub struct Extensions {
    /// The relay chain of the Parachain.
    pub relay_chain: String,
    /// The id of the Parachain.
    pub para_id: u32,
}

impl Extensions {
    /// Try to get the extension from the given `ChainSpec`.
    pub fn try_get(chain_spec: &Box<dyn sc_service::ChainSpec>) -> Option<&Self> {
        sc_chain_spec::get_extension(chain_spec.extensions())
    }
}

/// Specialized `ChainSpec`.
pub type ChainSpec = sc_service::GenericChainSpec<GenesisConfig, Extensions>;

pub fn get_chain_spec(id: ParaId) -> ChainSpec {
    if id == ParaId::from(EARTH_ID) {
        return earth_parachain_config();
    }

    if id == ParaId::from(MARS_ID) {
        return mars_parachain_config();
    }

    if id == ParaId::from(VENUS_ID) {
        return venus_parachain_config();
    }

    if id == ParaId::from(URANUS_ID) {
        return uranus_parachain_config();
    }

    #[cfg(feature = "kusama-parachain")]
    if id == ParaId::from(KUSAMA_ID) {
        return kusama_parachain_config();
    }

    test_chain_spec(id)
}

fn test_chain_spec(id: ParaId) -> ChainSpec {
    let balances = vec![
        get_account_id_from_seed::<sr25519::Public>("Alice"),
        get_account_id_from_seed::<sr25519::Public>("Bob"),
        get_account_id_from_seed::<sr25519::Public>("Charlie"),
        get_account_id_from_seed::<sr25519::Public>("Dave"),
        get_account_id_from_seed::<sr25519::Public>("Eve"),
        get_account_id_from_seed::<sr25519::Public>("Ferdie"),
    ];
    ChainSpec::from_genesis(
        "Local Testnet",
        "local_testnet",
        ChainType::Local,
        move || {
            mk_genesis(
                balances
                    .iter()
                    .cloned()
                    .map(|a| (a, 1_000_000_000_000u128))
                    .collect(),
                get_account_id_from_seed::<sr25519::Public>("Alice"),
                wasm_binary_unwrap().to_vec(),
                id,
            )
        },
        vec![],
        None,
        None,
        None,
        Extensions {
            relay_chain: "westend-dev".into(),
            para_id: id.into(),
        },
    )
}

/// Helper function to create GenesisConfig for parachain
fn mk_genesis(
    balances: Vec<(AccountId, Balance)>,
    sudo_key: AccountId,
    code: Vec<u8>,
    parachain_id: ParaId,
) -> GenesisConfig {
    let bonus = balances.clone();
    GenesisConfig {
        frame_system: SystemConfig {
            code,
            changes_trie_config: Default::default(),
        },
        pallet_balances: BalancesConfig { balances },
        pallet_elections_phragmen: Default::default(),
        pallet_collective_Instance1: Default::default(),
        pallet_treasury: Default::default(),
        pallet_robonomics_staking: StakingConfig { bonus },
        pallet_sudo: SudoConfig { key: sudo_key },
        parachain_info: ParachainInfoConfig { parachain_id },
    }
}

const STAGING_TELEMETRY_URL: &str = "wss://telemetry.polkadot.io/submit/";
const ROBONOMICS_PROTOCOL_ID: &str = "xrt";

/*
/// Earth parachain genesis.
fn earth_parachain_genesis() -> GenesisConfig {
    use alpha_runtime::constants::currency;
    use hex_literal::hex;

    // akru
    let sudo_key: AccountId =
        hex!["16eb796bee0c857db3d646ee7070252707aec0c7d82b2eda856632f6a2306a58"].into();

    let mut balances = currency::STAKE_HOLDERS.clone();
    balances.extend(vec![(sudo_key.clone(), 50_000 * currency::XRT)]);

    mk_genesis(
        balances.to_vec(),
        sudo_key,
        wasm_binary_unwrap().to_vec(),
        EARTH_ID.into(),
    )
}

/// Earth parachain config.
pub fn earth_parachain_config() -> ChainSpec {
    let boot_nodes = vec![];
    ChainSpec::from_genesis(
        "Earth",
        "earth",
        ChainType::Live,
        earth_parachain_genesis,
        boot_nodes,
        Some(
            sc_telemetry::TelemetryEndpoints::new(vec![(STAGING_TELEMETRY_URL.to_string(), 0)])
                .unwrap(),
        ),
        Some(ROBONOMICS_PROTOCOL_ID),
        None,
        Extensions {
            relay_chain: "rococo_local_testnet".into(),
            para_id: EARTH_ID.into(),
        },
    )
}

/// Mars parachain genesis.
fn mars_parachain_genesis() -> GenesisConfig {
    use alpha_runtime::constants::currency;
    use hex_literal::hex;

    // akru
    let sudo_key: AccountId =
        hex!["16eb796bee0c857db3d646ee7070252707aec0c7d82b2eda856632f6a2306a58"].into();

    let balances = currency::STAKE_HOLDERS.clone();
    mk_genesis(
        balances.to_vec(),
        sudo_key,
        wasm_binary_unwrap().to_vec(),
        MARS_ID.into(),
    )
}

/// Mars parachain config.
pub fn mars_parachain_config() -> ChainSpec {
    let boot_nodes = vec![];
    ChainSpec::from_genesis(
        "Mars",
        "mars",
        ChainType::Live,
        mars_parachain_genesis,
        boot_nodes,
        Some(
            sc_telemetry::TelemetryEndpoints::new(vec![(STAGING_TELEMETRY_URL.to_string(), 0)])
                .unwrap(),
        ),
        Some(ROBONOMICS_PROTOCOL_ID),
        None,
        Extensions {
            relay_chain: "rococo_local_testnet".into(),
            para_id: MARS_ID.into(),
        },
    )
}
*/

/// Kusama parachain genesis.
fn kusama_parachain_genesis() -> GenesisConfig {
    use alpha_runtime::constants::currency;
    use hex_literal::hex;

    // akru
    let sudo_key: AccountId =
        hex!["16eb796bee0c857db3d646ee7070252707aec0c7d82b2eda856632f6a2306a58"].into();

    let balances = currency::STAKE_HOLDERS.clone();
    mk_genesis(
        balances.to_vec(),
        sudo_key,
        wasm_binary_unwrap().to_vec(),
        KUSAMA_ID.into(),
    )
}

/// Kusama parachain config.
pub fn kusama_parachain_config() -> ChainSpec {
    let boot_nodes = vec![];
    ChainSpec::from_genesis(
        "Robonomics",
        "robonomics",
        ChainType::Live,
        kusama_parachain_genesis,
        boot_nodes,
        Some(
            sc_telemetry::TelemetryEndpoints::new(vec![(STAGING_TELEMETRY_URL.to_string(), 0)])
                .unwrap(),
        ),
        Some(ROBONOMICS_PROTOCOL_ID),
        None,
        Extensions {
            relay_chain: "kusama".into(),
            para_id: KUSAMA_ID.into(),
        },
    )
}

/// Earth parachain confing.
pub fn earth_parachain_config() -> ChainSpec {
    ChainSpec::from_json_bytes(&include_bytes!("../../res/earth.json")[..]).unwrap()
}

/// Mars parachain confing.
pub fn mars_parachain_config() -> ChainSpec {
    ChainSpec::from_json_bytes(&include_bytes!("../../res/mars.json")[..]).unwrap()
}

/// Venus parachain confing.
pub fn venus_parachain_config() -> ChainSpec {
    ChainSpec::from_json_bytes(&include_bytes!("../../res/venus.json")[..]).unwrap()
}

/// Uranus parachain confing.
pub fn uranus_parachain_config() -> ChainSpec {
    ChainSpec::from_json_bytes(&include_bytes!("../../res/uranus.json")[..]).unwrap()
}

/*
/// Rococo parachain confing.
#[cfg(feature = "kusama-parachain")]
pub fn kusama_parachain_config() -> ChainSpec {
    ChainSpec::from_json_bytes(&include_bytes!("../../res/robonomics.json")[..]).unwrap()
}
*/
