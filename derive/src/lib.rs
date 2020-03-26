#![deny(missing_docs, unsafe_code)]

//! Implementation of procedural macro for generating type-safe bindings to an
//! ethereum smart contract.

extern crate proc_macro;

mod spanned;

use crate::spanned::{ParseInner, Spanned};
use ethcontract_generate::{parse_address, Address, Builder};
use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use std::collections::HashSet;
use std::error::Error;
use syn::ext::IdentExt;
use syn::parse::{Error as ParseError, Parse, ParseStream, Result as ParseResult};
use syn::{braced, parse_macro_input, Error as SynError, Ident, LitInt, LitStr, Token};

/// Proc macro to generate type-safe bindings to a contract. This macro accepts
/// a path to a Truffle artifact JSON file. Note that this path is rooted in
/// the crate's root `CARGO_MANIFEST_DIR`.
///
/// ```ignore
/// ethcontract::contract!("build/contracts/MyContract.json");
/// ```
///
/// Alternatively, an etherscan URL can be specified. In this case the ABI will
/// be retrieved and used to generate type-safe contract bindings:
///
/// ```ignore
/// ethcontract::contract!("etherscan:0x0001020304050607080910111213141516171819");
/// // or
/// ethcontract::contract!("https://etherscan.io/address/0x0001020304050607080910111213141516171819");
/// ```
///
/// Note that Etherscan rate-limits requests to their API, to avoid this an
/// `ETHERSCAN_API_KEY` environment variable can be set. If it is, it will use
/// that API key when retrieving the contract ABI.
///
/// Currently the proc macro accepts additional parameters to configure some
/// aspects of the code generation. Specifically it accepts:
/// - `crate`: The name of the `ethcontract` crate. This is useful if the crate
///   was renamed in the `Cargo.toml` for whatever reason.
/// - `contract`: Override the contract name that is used for the generated
///   type. This is required when using sources that do not provide the contract
///   name in the artifact JSON such as Etherscan.
/// - `deployments`: A list of additional addresses of deployed contract for
///   specified network IDs. This mapping allows `MyContract::deployed` to work
///   for networks that are not included in the Truffle artifact's `networks`
///   property. Note that deployments defined this way **take precedence** over
///   the ones defined in the Truffle artifact. This parameter is intended to be
///   used to manually specify contract addresses for test environments, be it
///   testnet addresses that may defer from the originally published artifact or
///   deterministic contract addresses on local development nodes.
///
/// ```ignore
/// ethcontract::contract!(
///     "build/contracts/MyContract.json",
///     crate = ethcontract_rename,
///     contract = MyContractInstance,
///     deployments {
///         4 => "0x000102030405060708090a0b0c0d0e0f10111213"
///         5777 => "0x0123456789012345678901234567890123456789"
///     },
/// );
/// ```
///
/// See [`ethcontract`](ethcontract) module level documentation for additional
/// information.
#[proc_macro]
pub fn contract(input: TokenStream) -> TokenStream {
    let args = parse_macro_input!(input as Spanned<ContractArgs>);

    let span = args.span();
    expand(args.into_inner())
        .unwrap_or_else(|e| SynError::new(span, format!("{:?}", e)).to_compile_error())
        .into()
}

fn expand(args: ContractArgs) -> Result<TokenStream2, Box<dyn Error>> {
    Ok(args.into_builder()?.generate()?.into_tokens())
}

/// Contract procedural macro arguments.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
struct ContractArgs {
    artifact_path: String,
    parameters: Vec<Parameter>,
}

impl ContractArgs {
    fn into_builder(self) -> Result<Builder, Box<dyn Error>> {
        let mut builder = Builder::from_source_url(&self.artifact_path)?;
        for parameter in self.parameters.into_iter() {
            builder = match parameter {
                Parameter::Crate(name) => builder.with_runtime_crate_name(name),
                Parameter::Contract(name) => builder.with_contract_name_override(Some(name)),
                Parameter::Deployments(deployments) => {
                    deployments.into_iter().fold(builder, |builder, d| {
                        builder.add_deployment(d.network_id, d.address)
                    })
                }
            };
        }

        Ok(builder)
    }
}

impl ParseInner for ContractArgs {
    fn spanned_parse(input: ParseStream) -> ParseResult<(Span, Self)> {
        // TODO(nlordell): Due to limitation with the proc-macro Span API, we
        //   can't currently get a path the the file where we were called from;
        //   therefore, the path will always be rooted on the cargo manifest
        //   directory. Eventually we can use the `Span::source_file` API to
        //   have a better experience.
        let (span, artifact_path) = {
            let literal = input.parse::<LitStr>()?;
            (literal.span(), literal.value())
        };

        if !input.is_empty() {
            input.parse::<Token![,]>()?;
        }
        let parameters = input
            .parse_terminated::<_, Token![,]>(Parameter::parse)?
            .into_iter()
            .collect();

        Ok((
            span,
            ContractArgs {
                artifact_path,
                parameters,
            },
        ))
    }
}

/// A single procedural macro parameter.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
enum Parameter {
    Crate(String),
    Contract(String),
    Deployments(Vec<Deployment>),
}

impl Parse for Parameter {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let name = input.call(Ident::parse_any)?;
        let param = match name.to_string().as_str() {
            "crate" => {
                input.parse::<Token![=]>()?;
                let name = input.call(Ident::parse_any)?.to_string();

                Parameter::Crate(name)
            }
            "contract" => {
                input.parse::<Token![=]>()?;
                let name = input.parse::<Ident>()?.to_string();

                Parameter::Contract(name)
            }
            "deployments" => {
                let content;
                braced!(content in input);
                let deployments = {
                    let parsed =
                        content.parse_terminated::<_, Token![,]>(Spanned::<Deployment>::parse)?;

                    let mut deployments = Vec::with_capacity(parsed.len());
                    let mut networks = HashSet::new();
                    for deployment in parsed {
                        if !networks.insert(deployment.network_id) {
                            return Err(ParseError::new(
                                deployment.span(),
                                "duplicate network ID in `ethcontract::contract!` macro invocation",
                            ));
                        }
                        deployments.push(deployment.into_inner())
                    }

                    deployments
                };

                Parameter::Deployments(deployments)
            }
            _ => {
                return Err(ParseError::new(
                    name.span(),
                    format!("unexpected named parameter `{}`", name),
                ))
            }
        };

        Ok(param)
    }
}

/// A manually specified dependency.
#[cfg_attr(test, derive(Debug, Eq, PartialEq))]
struct Deployment {
    network_id: u32,
    address: Address,
}

impl Parse for Deployment {
    fn parse(input: ParseStream) -> ParseResult<Self> {
        let network_id = input.parse::<LitInt>()?.base10_parse()?;
        input.parse::<Token![=>]>()?;
        let address = {
            let literal = input.parse::<LitStr>()?;
            parse_address(&literal.value()).map_err(|err| ParseError::new(literal.span(), err))?
        };

        Ok(Deployment {
            network_id,
            address,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    macro_rules! contract_args_result {
        ($($arg:tt)*) => {{
            use syn::parse::Parser;
            <Spanned<ContractArgs> as Parse>::parse
                .parse2(quote::quote! { $($arg)* })
        }};
    }
    macro_rules! contract_args {
        ($($arg:tt)*) => {
            contract_args_result!($($arg)*)
                .expect("failed to parse contract args")
                .into_inner()
        };
    }
    macro_rules! contract_args_err {
        ($($arg:tt)*) => {
            contract_args_result!($($arg)*)
                .expect_err("expected parse contract args to error")
        };
    }

    fn deployment(network_id: u32, address: &str) -> Deployment {
        Deployment {
            network_id,
            address: parse_address(address).expect("failed to parse deployment address"),
        }
    }

    #[test]
    fn parse_contract_args() {
        let args = contract_args!("path/to/artifact.json");
        assert_eq!(args.artifact_path, "path/to/artifact.json");
    }

    #[test]
    fn crate_parameter_accepts_keywords() {
        let args = contract_args!("artifact.json", crate = crate);
        assert_eq!(args.parameters, &[Parameter::Crate("crate".into())]);
    }

    #[test]
    fn parse_contract_args_with_parameters() {
        let args = contract_args!(
            "artifact.json",
            crate = foobar,
            contract = Contract,
            deployments {
                1 => "0x000102030405060708090a0b0c0d0e0f10111213",
                4 => "0x0123456789012345678901234567890123456789",
            },
        );
        assert_eq!(
            args,
            ContractArgs {
                artifact_path: "artifact.json".into(),
                parameters: vec![
                    Parameter::Crate("foobar".into()),
                    Parameter::Contract("Contract".into()),
                    Parameter::Deployments(vec![
                        deployment(1, "0x000102030405060708090a0b0c0d0e0f10111213"),
                        deployment(4, "0x0123456789012345678901234567890123456789"),
                    ]),
                ],
            },
        );
    }

    #[test]
    fn duplicate_network_id_error() {
        contract_args_err!(
            "artifact.json",
            deployments {
                1 => "0x000102030405060708090a0b0c0d0e0f10111213",
                1 => "0x0123456789012345678901234567890123456789",
            }
        );
    }
}
