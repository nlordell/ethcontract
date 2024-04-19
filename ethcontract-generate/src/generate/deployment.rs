use crate::generate::{methods, Context};
use crate::util;
use anyhow::{Context as _, Result};
use inflector::Inflector;
use proc_macro2::{Literal, TokenStream};
use quote::quote;

pub(crate) fn expand(cx: &Context) -> Result<TokenStream> {
    let deployed = expand_deployed(cx);
    let deploy =
        expand_deploy(cx).context("error generating contract `deploy` associated function")?;

    Ok(quote! {
        #deployed
        #deploy
    })
}

fn expand_deployed(cx: &Context) -> TokenStream {
    if cx.contract.networks.is_empty() && cx.networks.is_empty() {
        if cx.contract.name == "DeployedContract" {
            println!("{:?}", cx.contract);
            println!("{:?}", cx.networks);
        }
        return quote! {};
    }

    quote! {
        impl Contract {
            /// Locates a deployed contract based on the current network ID
            /// reported by the `web3` provider.
            ///
            /// Note that this does not verify that a contract with a matching
            /// `Abi` is actually deployed at the given address.
            pub async fn deployed<F, B, T>(
                web3: &self::ethcontract::web3::api::Web3<T>,
            ) -> Result<Self, self::ethcontract::errors::DeployError>
            where
                F: std::future::Future<
                        Output = Result<
                            self::ethcontract::json::Value,
                            self::ethcontract::web3::Error,
                        >,
                    > + Send
                    + 'static,
                B: std::future::Future<
                        Output = Result<
                            Vec<
                                Result<
                                    self::ethcontract::json::Value,
                                    self::ethcontract::web3::Error,
                                >,
                            >,
                            self::ethcontract::web3::Error,
                        >,
                    > + Send
                    + 'static,
                T: self::ethcontract::web3::Transport<Out = F>
                    + self::ethcontract::web3::BatchTransport<Batch = B>
                    + Send
                    + Sync
                    + 'static,
            {
                use self::ethcontract::{Instance, Web3};
                use self::ethcontract::transport::DynTransport;

                let transport = DynTransport::new(web3.transport().clone());
                let web3 = Web3::new(transport);
                let instance = Instance::deployed(web3, Contract::raw_contract().clone()).await?;

                Ok(Contract::from_raw(instance))
            }
        }
    }
}

fn expand_deploy(cx: &Context) -> Result<TokenStream> {
    if cx.contract.bytecode.is_empty() {
        // do not generate deploy method for contracts that have empty bytecode
        return Ok(quote! {});
    }

    // TODO(nlordell): not sure how constructor documentation get generated as I
    //   can't seem to get truffle to output it
    let doc = util::expand_doc("Generated by `ethcontract`");

    let (input, arg) = match cx.contract.abi.abi.constructor() {
        Some(constructor) => (
            methods::expand_inputs(&constructor.inputs)?,
            methods::expand_inputs_call_arg(&constructor.inputs),
        ),
        None => (quote! {}, quote! {()}),
    };

    let libs: Vec<_> = cx
        .contract
        .bytecode
        .undefined_libraries()
        .map(|name| (name, util::safe_ident(&name.to_snake_case())))
        .collect();
    let (lib_struct, lib_input, link) = if !libs.is_empty() {
        let lib_struct = {
            let lib_struct_fields = libs.iter().map(|(name, field)| {
                let doc = util::expand_doc(&format!("Address of the `{}` library.", name));

                quote! {
                    #doc pub #field: self::ethcontract::Address
                }
            });

            quote! {
                /// Undefined libraries in the contract bytecode that are
                /// required for linking in order to deploy.
                pub struct Libraries {
                    #( #lib_struct_fields, )*
                }
            }
        };

        let link = {
            let link_libraries = libs.iter().map(|(name, field)| {
                let name_lit = Literal::string(name);

                quote! {
                    bytecode.link(#name_lit, libs.#field).expect("valid library");
                }
            });

            quote! {
                let mut bytecode = bytecode;
                #( #link_libraries )*
            }
        };

        (lib_struct, quote! { , libs: Libraries }, link)
    } else {
        Default::default()
    };

    Ok(quote! {
        #lib_struct

        impl Contract {
            #doc
            #[allow(clippy::too_many_arguments)]
            pub fn builder<F, B, T>(
                web3: &self::ethcontract::web3::api::Web3<T> #lib_input #input ,
            ) -> self::ethcontract::dyns::DynDeployBuilder<Self>
            where
                F: std::future::Future<
                        Output = Result<
                            self::ethcontract::json::Value,
                            self::ethcontract::web3::Error,
                        >,
                    > + Send
                    + 'static,
                B: std::future::Future<
                        Output = Result<
                            Vec<
                                Result<
                                    self::ethcontract::json::Value,
                                    self::ethcontract::web3::Error,
                                >,
                            >,
                            self::ethcontract::web3::Error,
                        >,
                    > + Send
                    + 'static,
                T: self::ethcontract::web3::Transport<Out = F>
                    + self::ethcontract::web3::BatchTransport<Batch = B>
                    + Send
                    + Sync
                    + 'static,
            {
                use self::ethcontract::dyns::DynTransport;
                use self::ethcontract::contract::DeployBuilder;
                use self::ethcontract::web3::api::Web3;

                let transport = DynTransport::new(web3.transport().clone());
                let web3 = Web3::new(transport);

                let bytecode = Self::raw_contract().bytecode.clone();
                #link

                DeployBuilder::new(web3, bytecode, #arg).expect("valid deployment args")
            }
        }

        impl self::ethcontract::contract::Deploy<self::ethcontract::dyns::DynTransport> for Contract {
            type Context = self::ethcontract::common::Bytecode;

            fn bytecode(cx: &Self::Context) -> &self::ethcontract::common::Bytecode {
                cx
            }

            fn abi(_: &Self::Context) -> &self::ethcontract::common::Abi {
                &Self::raw_contract().abi.abi
            }

            fn from_deployment(
                web3: self::ethcontract::dyns::DynWeb3,
                address: self::ethcontract::Address,
                transaction_hash: self::ethcontract::H256,
                _: Self::Context,
            ) -> Self {
                Self::with_deployment_info(&web3, address, Some(transaction_hash.into()))
            }
        }
    })
}
