use crate::contract::Context;
use crate::util::expand_doc;
use anyhow::Result;
use proc_macro2::TokenStream;
use quote::quote;

pub(crate) fn expand(cx: &Context) -> Result<TokenStream> {
    let ethcontract = &cx.runtime_crate;
    let artifact_json = &cx.artifact_json;
    let contract_name = &cx.contract_name;
    let methods = cx.methods_struct_name()?;

    let doc_str = cx
        .artifact
        .devdoc
        .details
        .as_deref()
        .unwrap_or("Generated by `ethcontract`");
    let doc = expand_doc(doc_str);

    Ok(quote! {
        #doc
        #[allow(non_camel_case_types)]
        #[derive(Clone)]
        pub struct #contract_name {
            methods: #methods,
        }

        impl #contract_name {
            /// Retrieves the truffle artifact used to generate the type safe
            /// API for this contract.
            pub fn artifact() -> &'static #ethcontract::Artifact {
                use #ethcontract::private::lazy_static;
                use #ethcontract::Artifact;

                lazy_static! {
                    pub static ref ARTIFACT: Artifact = {
                        Artifact::from_json(#artifact_json)
                            .expect("valid artifact JSON")
                    };
                }
                &ARTIFACT
            }

            /// Creates a new contract instance with the specified `web3`
            /// provider at the given `Address`.
            ///
            /// Note that this does not verify that a contract with a maching
            /// `Abi` is actually deployed at the given address.
            pub fn at<F, T>(
                web3: &#ethcontract::web3::api::Web3<T>,
                address: #ethcontract::Address,
            ) -> Self
            where
                F: #ethcontract::web3::futures::Future<Item = #ethcontract::json::Value, Error = #ethcontract::web3::Error> + Send + 'static,
                T: #ethcontract::web3::Transport<Out = F> + Send + Sync + 'static,
            {
                use #ethcontract::Instance;
                use #ethcontract::transport::DynTransport;
                use #ethcontract::web3::api::Web3;

                let transport = DynTransport::new(web3.transport().clone());
                let web3 = Web3::new(transport);
                let abi = Self::artifact().abi.clone();
                let instance = Instance::at(web3, abi, address);
                let methods = #methods { instance };

                #contract_name { methods }
            }

            /// Returns the contract address being used by this instance.
            pub fn address(&self) -> #ethcontract::Address {
                self.raw_instance().address()
            }

            /// Returns a reference to the default method options used by this
            /// contract.
            pub fn defaults(&self) -> &#ethcontract::contract::MethodDefaults {
                &self.raw_instance().defaults
            }

            /// Returns a mutable reference to the default method options used
            /// by this contract.
            pub fn defaults_mut(&mut self) -> &mut #ethcontract::contract::MethodDefaults {
                &mut self.raw_instance_mut().defaults
            }

            /// Returns a reference to the raw runtime instance used by this
            /// contract.
            pub fn raw_instance(&self) -> &#ethcontract::DynInstance {
                &self.methods.instance
            }

            /// Returns a mutable reference to the raw runtime instance used by
            /// this contract.
            fn raw_instance_mut(&mut self) -> &mut #ethcontract::DynInstance {
                &mut self.methods.instance
            }
        }

        impl std::fmt::Debug for #contract_name {
            fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                f.debug_tuple(stringify!(#contract_name))
                    .field(&self.address())
                    .finish()
            }
        }
    })
}
