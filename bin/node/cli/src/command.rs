///////////////////////////////////////////////////////////////////////////////
//
//  Copyright 2018-2020 Airalab <research@aira.life>
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

#[cfg(feature = "parachain")]
use crate::parachain::{
    chain_spec as parachain_spec, command as parachain_command, executor as parachain_executor,
};
use crate::{
    chain_spec::*,
    service::{executor, ipci, robonomics},
    Cli, Subcommand,
};
use sc_cli::{ChainSpec, Role, RuntimeVersion, SubstrateCli};

impl SubstrateCli for Cli {
    fn impl_name() -> &'static str {
        "airalab-robonomics"
    }

    fn impl_version() -> &'static str {
        env!("SUBSTRATE_CLI_IMPL_VERSION")
    }

    fn description() -> &'static str {
        env!("CARGO_PKG_DESCRIPTION")
    }

    fn author() -> &'static str {
        env!("CARGO_PKG_AUTHORS")
    }

    fn support_url() -> &'static str {
        "https://github.com/airalab/robonomics/issues/new"
    }

    fn copyright_start_year() -> i32 {
        2018
    }

    fn executable_name() -> &'static str {
        "robonomics"
    }

    fn load_spec(&self, id: &str) -> std::result::Result<Box<dyn sc_service::ChainSpec>, String> {
        Ok(match id {
            "dev" => Box::new(development_config()),
            "ipci" => Box::new(ipci_config()),
            #[cfg(feature = "parachain")]
            "" | "parachain" => Box::new(parachain_spec::robonomics_parachain_config()),
            path => Box::new(crate::chain_spec::ChainSpec::from_json_file(
                std::path::PathBuf::from(path),
            )?),
        })
    }

    fn native_runtime_version(chain_spec: &Box<dyn ChainSpec>) -> &'static RuntimeVersion {
        match chain_spec.family() {
            RobonomicsFamily::DaoIpci => &ipci_runtime::VERSION,
            RobonomicsFamily::Development => &robonomics_runtime::VERSION,
            #[cfg(feature = "parachain")]
            RobonomicsFamily::Parachain => &robonomics_parachain_runtime::VERSION,
            RobonomicsFamily::Unknown => panic!("Unknown runtime"),
        }
    }
}

/// Parse command line arguments into service configuration.
pub fn run() -> sc_cli::Result<()> {
    let cli = Cli::from_args();

    match &cli.subcommand {
        None => {
            let runner = cli.create_runner(&cli.run)?;
            match runner.config().chain_spec.family() {
                RobonomicsFamily::DaoIpci => {
                    runner.run_node_until_exit(|config| match config.role {
                        Role::Light => ipci::new_light(config),
                        _ => ipci::new_full(config),
                    })
                }

                RobonomicsFamily::Development => {
                    runner.run_node_until_exit(|config| match config.role {
                        Role::Light => robonomics::new_light(config),
                        _ => robonomics::new_full(config),
                    })
                }

                #[cfg(feature = "parachain")]
                RobonomicsFamily::Parachain => runner.run_node_until_exit(|config| {
                    if matches!(config.role, Role::Light) {
                        return Err("Light client not supporter!".into());
                    }

                    parachain_command::run(config, cli.parachain_id, &cli.relaychain_args)
                }),

                _ => Err(format!(
                    "unsupported chain spec: {}",
                    runner.config().chain_spec.id()
                ))?,
            }
        }
        Some(Subcommand::Base(subcommand)) => {
            let runner = cli.create_runner(subcommand)?;
            match runner.config().chain_spec.family() {
                RobonomicsFamily::DaoIpci => runner.run_subcommand(subcommand, |config| {
                    Ok(new_full_start!(config, ipci_runtime::RuntimeApi, executor::Ipci).0)
                }),

                RobonomicsFamily::Development => runner.run_subcommand(subcommand, |config| {
                    Ok(new_full_start!(
                        config,
                        robonomics_runtime::RuntimeApi,
                        executor::Robonomics
                    )
                    .0)
                }),

                #[cfg(feature = "parachain")]
                RobonomicsFamily::Parachain => runner.run_subcommand(subcommand, |config| {
                    Ok(new_parachain!(
                        config,
                        robonomics_parachain_runtime::RuntimeApi,
                        parachain_executor::Robonomics
                    )
                    .0)
                }),

                _ => Err(format!(
                    "unsupported chain spec: {}",
                    runner.config().chain_spec.id()
                ))?,
            }
        }
        #[cfg(feature = "robonomics-cli")]
        Some(Subcommand::Io(subcommand)) => {
            let runner = cli.create_runner(subcommand)?;
            runner.sync_run(|_| subcommand.run().map_err(|e| e.to_string().into()))
        }
        #[cfg(feature = "benchmarking-cli")]
        Some(Subcommand::Benchmark(subcommand)) => {
            let runner = cli.create_runner(subcommand)?;
            if runner.config().chain_spec.is_ipci() {
                runner.sync_run(|config| {
                    subcommand.run::<node_primitives::Block, executor::Ipci>(config)
                })
            } else {
                runner.sync_run(|config| {
                    subcommand.run::<node_primitives::Block, executor::Robonomics>(config)
                })
            }
        }
    }
}
