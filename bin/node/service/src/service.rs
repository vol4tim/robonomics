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
//! Service and ServiceFactory implementation. Specialized wrapper over Substrate service.

use robonomics_primitives::{AccountId, Balance, Block, Index};
use sc_client_api::{ExecutorProvider, RemoteBackend};
use sc_consensus_babe;
use sc_finality_grandpa::{self as grandpa, FinalityProofProvider as GrandpaFinalityProofProvider};
use sc_network::NetworkService;
use sc_service::{config::Configuration, error::Error as ServiceError, RpcHandlers, TaskManager};
use sp_api::ConstructRuntimeApi;
use sp_runtime::traits::{BlakeTwo256, Block as BlockT};
use std::sync::Arc;

type FullClient<Runtime, Executor> = sc_service::TFullClient<Block, Runtime, Executor>;
type FullBackend = sc_service::TFullBackend<Block>;
type FullSelectChain = sc_consensus::LongestChain<FullBackend, Block>;
type FullGrandpaBlockImport<Runtime, Executor> =
    grandpa::GrandpaBlockImport<FullBackend, Block, FullClient<Runtime, Executor>, FullSelectChain>;
type LightBackend = sc_service::TLightBackendWithHash<Block, BlakeTwo256>;
type LightClient<Runtime, Executor> =
    sc_service::TLightClientWithBackend<Block, Runtime, Executor, LightBackend>;

/// A set of APIs that robonomics-like runtimes must implement.
pub trait RuntimeApiCollection:
    sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>
    + sp_api::ApiExt<Block>
    + sp_consensus_babe::BabeApi<Block>
    + sp_finality_grandpa::GrandpaApi<Block>
    + sp_block_builder::BlockBuilder<Block>
    + pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance>
    + frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index>
    + sp_api::Metadata<Block>
    + sp_offchain::OffchainWorkerApi<Block>
    + sp_session::SessionKeys<Block>
where
    <Self as sp_api::ApiExt<Block>>::StateBackend: sp_api::StateBackend<BlakeTwo256>,
{
}

impl<Api> RuntimeApiCollection for Api
where
    Api: sp_transaction_pool::runtime_api::TaggedTransactionQueue<Block>
        + sp_api::ApiExt<Block>
        + sp_consensus_babe::BabeApi<Block>
        + sp_finality_grandpa::GrandpaApi<Block>
        + sp_block_builder::BlockBuilder<Block>
        + pallet_transaction_payment_rpc_runtime_api::TransactionPaymentApi<Block, Balance>
        + frame_system_rpc_runtime_api::AccountNonceApi<Block, AccountId, Index>
        + sp_api::Metadata<Block>
        + sp_offchain::OffchainWorkerApi<Block>
        + sp_session::SessionKeys<Block>,
    <Self as sp_api::ApiExt<Block>>::StateBackend: sp_api::StateBackend<BlakeTwo256>,
{
}

pub fn new_partial<Runtime, Executor>(
    config: &Configuration,
) -> Result<
    sc_service::PartialComponents<
        FullClient<Runtime, Executor>,
        FullBackend,
        FullSelectChain,
        sp_consensus::DefaultImportQueue<Block, FullClient<Runtime, Executor>>,
        sc_transaction_pool::FullPool<Block, FullClient<Runtime, Executor>>,
        (
            impl Fn(node_rpc::DenyUnsafe, sc_rpc::SubscriptionTaskExecutor) -> node_rpc::IoHandler,
            (
                sc_consensus_babe::BabeBlockImport<
                    Block,
                    FullClient<Runtime, Executor>,
                    FullGrandpaBlockImport<Runtime, Executor>,
                >,
                grandpa::LinkHalf<Block, FullClient<Runtime, Executor>, FullSelectChain>,
                sc_consensus_babe::BabeLink<Block>,
            ),
            grandpa::SharedVoterState,
            Option<sc_telemetry::Telemetry>,
        ),
    >,
    ServiceError,
>
where
    Runtime: ConstructRuntimeApi<Block, FullClient<Runtime, Executor>> + Send + Sync + 'static,
    Runtime::RuntimeApi:
        RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
    Executor: sc_executor::NativeExecutionDispatch + 'static,
{
    let telemetry = config
        .telemetry_endpoints
        .clone()
        .filter(|x| !x.is_empty())
        .map(|endpoints| -> Result<_, sc_telemetry::Error> {
            let worker = sc_telemetry::TelemetryWorker::new(16)?;
            let telemetry = worker.handle().new_telemetry(endpoints);
            Ok((worker, telemetry))
        })
        .transpose()?;
    let (client, backend, keystore_container, task_manager) =
        sc_service::new_full_parts::<Block, Runtime, Executor>(
            &config,
            telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
        )?;

    let client = Arc::new(client);
    let select_chain = sc_consensus::LongestChain::new(backend.clone());

    let telemetry = telemetry.map(|(worker, telemetry)| {
        task_manager.spawn_handle().spawn("telemetry", worker.run());
        telemetry
    });

    let transaction_pool = sc_transaction_pool::BasicPool::new_full(
        config.transaction_pool.clone(),
        config.role.is_authority().into(),
        config.prometheus_registry(),
        task_manager.spawn_handle(),
        client.clone(),
    );

    let (grandpa_block_import, grandpa_link) = grandpa::block_import(
        client.clone(),
        &(client.clone() as Arc<_>),
        select_chain.clone(),
        telemetry.as_ref().map(|x| x.handle()),
    )?;
    let justification_import = grandpa_block_import.clone();

    let (block_import, babe_link) = sc_consensus_babe::block_import(
        sc_consensus_babe::Config::get_or_compute(&*client)?,
        grandpa_block_import,
        client.clone(),
    )?;

    let slot_duration = babe_link.config().slot_duration();
    let import_queue = sc_consensus_babe::import_queue(
        babe_link.clone(),
        block_import.clone(),
        Some(Box::new(justification_import)),
        client.clone(),
        select_chain.clone(),
        move |_, ()| async move {
            let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
            let slot =
                sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_duration(
                    *timestamp,
                    slot_duration,
                );
            let uncles =
                sp_authorship::InherentDataProvider::<<Block as BlockT>::Header>::check_inherents();
            Ok((timestamp, slot, uncles))
        },
        &task_manager.spawn_essential_handle(),
        config.prometheus_registry(),
        sp_consensus::CanAuthorWithNativeVersion::new(client.executor().clone()),
        telemetry.as_ref().map(|x| x.handle()),
    )?;

    let import_setup = (block_import, grandpa_link, babe_link);

    let (rpc_extensions_builder, rpc_setup) = {
        let (_, grandpa_link, babe_link) = &import_setup;

        let justification_stream = grandpa_link.justification_stream();
        let shared_authority_set = grandpa_link.shared_authority_set().clone();
        let shared_voter_state = grandpa::SharedVoterState::empty();
        let rpc_setup = shared_voter_state.clone();

        let finality_proof_provider = GrandpaFinalityProofProvider::new_for_service(
            backend.clone(),
            Some(shared_authority_set.clone()),
        );

        let babe_config = babe_link.config().clone();
        let shared_epoch_changes = babe_link.epoch_changes().clone();

        let client = client.clone();
        let pool = transaction_pool.clone();
        let select_chain = select_chain.clone();
        let keystore = keystore_container.sync_keystore();
        let chain_spec = config.chain_spec.cloned_box();

        let rpc_extensions_builder = move |deny_unsafe, subscription_executor| {
            let deps = node_rpc::FullDeps {
                client: client.clone(),
                pool: pool.clone(),
                select_chain: select_chain.clone(),
                chain_spec: chain_spec.cloned_box(),
                deny_unsafe,
                babe: node_rpc::BabeDeps {
                    babe_config: babe_config.clone(),
                    shared_epoch_changes: shared_epoch_changes.clone(),
                    keystore: keystore.clone(),
                },
                grandpa: node_rpc::GrandpaDeps {
                    shared_voter_state: shared_voter_state.clone(),
                    shared_authority_set: shared_authority_set.clone(),
                    justification_stream: justification_stream.clone(),
                    subscription_executor,
                    finality_provider: finality_proof_provider.clone(),
                },
            };

            node_rpc::create_full(deps)
        };

        (rpc_extensions_builder, rpc_setup)
    };

    Ok(sc_service::PartialComponents {
        client,
        backend,
        task_manager,
        keystore_container,
        select_chain,
        import_queue,
        transaction_pool,
        other: (rpc_extensions_builder, import_setup, rpc_setup, telemetry),
    })
}

/// Creates a full service from the configuration.
pub fn new_full_base<Runtime, Executor>(
    mut config: Configuration,
) -> Result<
    (
        TaskManager,
        Arc<FullClient<Runtime, Executor>>,
        Arc<NetworkService<Block, <Block as BlockT>::Hash>>,
        Arc<sc_transaction_pool::FullPool<Block, FullClient<Runtime, Executor>>>,
    ),
    ServiceError,
>
where
    Runtime: ConstructRuntimeApi<Block, FullClient<Runtime, Executor>> + Send + Sync + 'static,
    Runtime::RuntimeApi:
        RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<FullBackend, Block>>,
    Executor: sc_executor::NativeExecutionDispatch + 'static,
{
    let sc_service::PartialComponents {
        client,
        backend,
        mut task_manager,
        import_queue,
        keystore_container,
        select_chain,
        transaction_pool,
        other: (rpc_extensions_builder, import_setup, rpc_setup, mut telemetry),
    } = new_partial(&config)?;

    let shared_voter_state = rpc_setup;

    config
        .network
        .extra_sets
        .push(grandpa::grandpa_peers_set_config());

    #[cfg(feature = "cli")]
    config.network.request_response_protocols.push(
        sc_finality_grandpa_warp_sync::request_response_config_for_chain(
            &config,
            task_manager.spawn_handle(),
            backend.clone(),
            import_setup.1.shared_authority_set().clone(),
        ),
    );

    let (network, system_rpc_tx, network_starter) =
        sc_service::build_network(sc_service::BuildNetworkParams {
            config: &config,
            client: client.clone(),
            transaction_pool: transaction_pool.clone(),
            spawn_handle: task_manager.spawn_handle(),
            import_queue,
            on_demand: None,
            block_announce_validator_builder: None,
        })?;

    if config.offchain_worker.enabled {
        sc_service::build_offchain_workers(
            &config,
            task_manager.spawn_handle(),
            client.clone(),
            network.clone(),
        );
    }

    let role = config.role.clone();
    let force_authoring = config.force_authoring;
    let backoff_authoring_blocks =
        Some(sc_consensus_slots::BackoffAuthoringOnFinalizedHeadLagging::default());
    let name = config.network.node_name.clone();
    let enable_grandpa = !config.disable_grandpa;
    let prometheus_registry = config.prometheus_registry().cloned();

    sc_service::spawn_tasks(sc_service::SpawnTasksParams {
        config,
        backend: backend.clone(),
        client: client.clone(),
        keystore: keystore_container.sync_keystore(),
        network: network.clone(),
        rpc_extensions_builder: Box::new(rpc_extensions_builder),
        transaction_pool: transaction_pool.clone(),
        task_manager: &mut task_manager,
        on_demand: None,
        remote_blockchain: None,
        system_rpc_tx,
        telemetry: telemetry.as_mut(),
    })?;

    let (block_import, grandpa_link, babe_link) = import_setup;

    if let sc_service::config::Role::Authority { .. } = &role {
        let proposer = sc_basic_authorship::ProposerFactory::new(
            task_manager.spawn_handle(),
            client.clone(),
            transaction_pool.clone(),
            prometheus_registry.as_ref(),
            telemetry.as_ref().map(|x| x.handle()),
        );

        let can_author_with =
            sp_consensus::CanAuthorWithNativeVersion::new(client.executor().clone());

        let client_clone = client.clone();
        let slot_duration = babe_link.config().slot_duration();
        let babe_config = sc_consensus_babe::BabeParams {
            keystore: keystore_container.sync_keystore(),
            client: client.clone(),
            select_chain,
            env: proposer,
            block_import,
            sync_oracle: network.clone(),
            create_inherent_data_providers: move |parent, ()| {
                let client_clone = client_clone.clone();
                async move {
                    let uncles = sc_consensus_uncles::create_uncles_inherent_data_provider(
                        &*client_clone,
                        parent,
                    )?;
                    let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
                    let slot =
                        sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_duration(
                            *timestamp,
                            slot_duration,
                        );
                    Ok((timestamp, slot, uncles))
                }
            },
            force_authoring,
            backoff_authoring_blocks,
            babe_link,
            can_author_with,
            block_proposal_slot_portion: sc_consensus_babe::SlotProportion::new(0.5),
            telemetry: telemetry.as_ref().map(|x| x.handle()),
        };

        let babe = sc_consensus_babe::start_babe(babe_config)?;
        task_manager
            .spawn_essential_handle()
            .spawn_blocking("babe-proposer", babe);
    }

    // if the node isn't actively participating in consensus then it doesn't
    // need a keystore, regardless of which protocol we use below.
    let keystore = if role.is_authority() {
        Some(keystore_container.sync_keystore())
    } else {
        None
    };

    let config = grandpa::Config {
        // FIXME #1578 make this available through chainspec
        gossip_duration: std::time::Duration::from_millis(333),
        justification_period: 512,
        name: Some(name),
        observer_enabled: false,
        local_role: role,
        keystore,
        telemetry: telemetry.as_ref().map(|x| x.handle()),
    };

    if enable_grandpa {
        // start the full GRANDPA voter
        // NOTE: non-authorities could run the GRANDPA observer protocol, but at
        // this point the full voter should provide better guarantees of block
        // and vote data availability than the observer. The observer has not
        // been tested extensively yet and having most nodes in a network run it
        // could lead to finality stalls.
        let grandpa_config = grandpa::GrandpaParams {
            config,
            link: grandpa_link,
            network: network.clone(),
            voting_rule: grandpa::VotingRulesBuilder::default().build(),
            prometheus_registry,
            shared_voter_state,
            telemetry: telemetry.as_ref().map(|x| x.handle()),
        };

        // the GRANDPA voter task is considered infallible, i.e.
        // if it fails we take down the service with it.
        task_manager
            .spawn_essential_handle()
            .spawn_blocking("grandpa-voter", grandpa::run_grandpa_voter(grandpa_config)?);
    }

    network_starter.start_network();
    Ok((task_manager, client, network, transaction_pool))
}

pub fn new_light_base<Runtime, Executor>(
    mut config: Configuration,
) -> Result<
    (
        TaskManager,
        RpcHandlers,
        Arc<LightClient<Runtime, Executor>>,
        Arc<NetworkService<Block, <Block as BlockT>::Hash>>,
        Arc<
            sc_transaction_pool::LightPool<
                Block,
                LightClient<Runtime, Executor>,
                sc_network::config::OnDemand<Block>,
            >,
        >,
    ),
    ServiceError,
>
where
    Runtime: ConstructRuntimeApi<Block, LightClient<Runtime, Executor>> + Send + Sync + 'static,
    Runtime::RuntimeApi:
        RuntimeApiCollection<StateBackend = sc_client_api::StateBackendFor<LightBackend, Block>>,
    Executor: sc_executor::NativeExecutionDispatch + 'static,
{
    let telemetry = config
        .telemetry_endpoints
        .clone()
        .filter(|x| !x.is_empty())
        .map(|endpoints| -> Result<_, sc_telemetry::Error> {
            #[cfg(feature = "browser")]
            let transport = Some(sc_telemetry::ExtTransport::new(
                libp2p_wasm_ext::ffi::websocket_transport(),
            ));
            #[cfg(not(feature = "browser"))]
            let transport = None;

            let worker = sc_telemetry::TelemetryWorker::with_transport(16, transport)?;
            let telemetry = worker.handle().new_telemetry(endpoints);
            Ok((worker, telemetry))
        })
        .transpose()?;

    let (client, backend, keystore_container, mut task_manager, on_demand) =
        sc_service::new_light_parts::<Block, Runtime, Executor>(
            &config,
            telemetry.as_ref().map(|(_, telemetry)| telemetry.handle()),
        )?;

    let mut telemetry = telemetry.map(|(worker, telemetry)| {
        task_manager.spawn_handle().spawn("telemetry", worker.run());
        telemetry
    });

    let select_chain = sc_consensus::LongestChain::new(backend.clone());

    config
        .network
        .extra_sets
        .push(grandpa::grandpa_peers_set_config());

    let transaction_pool = Arc::new(sc_transaction_pool::BasicPool::new_light(
        config.transaction_pool.clone(),
        config.prometheus_registry(),
        task_manager.spawn_handle(),
        client.clone(),
        on_demand.clone(),
    ));

    let (grandpa_block_import, _) = grandpa::block_import(
        client.clone(),
        &(client.clone() as Arc<_>),
        select_chain.clone(),
        telemetry.as_ref().map(|x| x.handle()),
    )?;
    let justification_import = grandpa_block_import.clone();

    let (babe_block_import, babe_link) = sc_consensus_babe::block_import(
        sc_consensus_babe::Config::get_or_compute(&*client)?,
        grandpa_block_import,
        client.clone(),
    )?;

    let slot_duration = babe_link.config().slot_duration();
    let import_queue = sc_consensus_babe::import_queue(
        babe_link,
        babe_block_import,
        Some(Box::new(justification_import)),
        client.clone(),
        select_chain.clone(),
        move |_, ()| async move {
            let timestamp = sp_timestamp::InherentDataProvider::from_system_time();
            let slot =
                sp_consensus_babe::inherents::InherentDataProvider::from_timestamp_and_duration(
                    *timestamp,
                    slot_duration,
                );
            let uncles =
                sp_authorship::InherentDataProvider::<<Block as BlockT>::Header>::check_inherents();
            Ok((timestamp, slot, uncles))
        },
        &task_manager.spawn_essential_handle(),
        config.prometheus_registry(),
        sp_consensus::NeverCanAuthor,
        telemetry.as_ref().map(|x| x.handle()),
    )?;

    let (network, system_rpc_tx, network_starter) =
        sc_service::build_network(sc_service::BuildNetworkParams {
            config: &config,
            client: client.clone(),
            transaction_pool: transaction_pool.clone(),
            spawn_handle: task_manager.spawn_handle(),
            import_queue,
            on_demand: Some(on_demand.clone()),
            block_announce_validator_builder: None,
        })?;
    network_starter.start_network();

    if config.offchain_worker.enabled {
        sc_service::build_offchain_workers(
            &config,
            task_manager.spawn_handle(),
            client.clone(),
            network.clone(),
        );
    }

    let light_deps = node_rpc::LightDeps {
        remote_blockchain: backend.remote_blockchain(),
        fetcher: on_demand.clone(),
        client: client.clone(),
        pool: transaction_pool.clone(),
    };

    let rpc_extensions = node_rpc::create_light(light_deps);

    let rpc_handlers = sc_service::spawn_tasks(sc_service::SpawnTasksParams {
        on_demand: Some(on_demand),
        remote_blockchain: Some(backend.remote_blockchain()),
        rpc_extensions_builder: Box::new(sc_service::NoopRpcExtensionBuilder(rpc_extensions)),
        client: client.clone(),
        transaction_pool: transaction_pool.clone(),
        keystore: keystore_container.sync_keystore(),
        config,
        backend,
        system_rpc_tx,
        network: network.clone(),
        task_manager: &mut task_manager,
        telemetry: telemetry.as_mut(),
    })?;

    Ok((
        task_manager,
        rpc_handlers,
        client,
        network,
        transaction_pool,
    ))
}

/// Robonomics chain services.
pub mod robonomics {
    use local_runtime::RuntimeApi;
    use sc_service::{config::Configuration, error::Result, RpcHandlers, TaskManager};

    #[cfg(feature = "frame-benchmarking")]
    sc_executor::native_executor_instance!(
        pub Executor,
        local_runtime::api::dispatch,
        local_runtime::native_version,
        frame_benchmarking::benchmarking::HostFunctions,
    );

    #[cfg(not(feature = "frame-benchmarking"))]
    sc_executor::native_executor_instance!(
        pub Executor,
        local_runtime::api::dispatch,
        local_runtime::native_version,
    );

    /// Create a new Robonomics service for a full node.
    pub fn new_full(config: Configuration) -> Result<TaskManager> {
        super::new_full_base::<RuntimeApi, Executor>(config)
            .map(|(task_manager, _, _, _)| task_manager)
    }

    pub fn new_light(config: Configuration) -> Result<(TaskManager, RpcHandlers)> {
        super::new_light_base::<RuntimeApi, Executor>(config)
            .map(|(task_manager, rpc_handlers, _, _, _)| (task_manager, rpc_handlers))
    }
}
