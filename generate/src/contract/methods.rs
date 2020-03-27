use crate::contract::{types, Context};
use crate::util;
use anyhow::{anyhow, Context as _, Result};
use ethcontract_common::abi::{Function, Param};
use ethcontract_common::abiext::FunctionExt;
use inflector::Inflector;
use proc_macro2::{Literal, TokenStream};
use quote::quote;
use syn::Ident;

pub(crate) fn expand(cx: &Context) -> Result<TokenStream> {
    let mut aliases = cx.method_aliases.clone();
    let functions = cx
        .artifact
        .abi
        .functions()
        .map(|function| {
            let signature = function.abi_signature();
            expand_function(&cx, function, aliases.remove(&signature))
                .with_context(|| format!("error expanding function '{}'", signature))
        })
        .collect::<Result<Vec<_>>>()?;
    if let Some(unused) = aliases.keys().next() {
        return Err(anyhow!(
            "a manual method alias for '{}' was specified but this method does not exist",
            unused,
        ));
    }

    let methods_struct = quote! {
        struct Methods {
            instance: self::ethcontract::private::DynInstance,
        }
    };

    if functions.is_empty() {
        // NOTE: The methods struct is still needed when there are no functions
        //   as it contains the the runtime instance. The code is setup this way
        //   so that the contract can implement `Deref` targetting the methods
        //   struct and, therefore, call the methods directly.
        return Ok(quote! { #methods_struct });
    }

    Ok(quote! {
        impl Contract {
            /// Retrives a reference to type containing all the generated
            /// contract methods. This can be used for methods where the name
            /// would collide with a common method (like `at` or `deployed`).
            pub fn methods(&self) -> &Methods {
                &self.methods
            }
        }

        /// Type containing all contract methods for generated contract type.
        #[allow(non_camel_case_types)]
        #[derive(Clone)]
        pub #methods_struct

        #[allow(clippy::too_many_arguments, clippy::type_complexity)]
        impl Methods {
            #( #functions )*
        }

        impl std::ops::Deref for Contract {
            type Target = Methods;
            fn deref(&self) -> &Self::Target {
                &self.methods
            }
        }
    })
}

fn expand_function(cx: &Context, function: &Function, alias: Option<Ident>) -> Result<TokenStream> {
    let name = alias.unwrap_or_else(|| util::safe_ident(&function.name.to_snake_case()));
    let signature = function.abi_signature();
    let signature_lit = Literal::string(&signature);

    let doc_str = cx
        .artifact
        .devdoc
        .methods
        .get(&signature)
        .or_else(|| cx.artifact.userdoc.methods.get(&signature))
        .and_then(|entry| entry.details.as_ref())
        .map(String::as_str)
        .unwrap_or("Generated by `ethcontract`");
    let doc = util::expand_doc(doc_str);

    let input = expand_inputs(&function.inputs)?;
    let outputs = expand_fn_outputs(&function.outputs)?;
    let (method, result_type_name) = if function.constant {
        (quote! { view_method }, quote! { DynViewMethodBuilder })
    } else {
        (quote! { method }, quote! { DynMethodBuilder })
    };
    let result = quote! { self::ethcontract::private::#result_type_name<#outputs> };
    let arg = expand_inputs_call_arg(&function.inputs);

    Ok(quote! {
        #doc
        pub fn #name(&self #input) -> #result {
            self.instance.#method(#signature_lit, #arg)
                .expect("generated call")
        }
    })
}

pub(crate) fn expand_inputs(inputs: &[Param]) -> Result<TokenStream> {
    let params = inputs
        .iter()
        .enumerate()
        .map(|(i, param)| {
            let name = expand_input_name(i, &param.name);
            let kind = types::expand(&param.kind)?;
            Ok(quote! { #name: #kind })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(quote! { #( , #params )* })
}

fn input_name_to_ident(index: usize, name: &str) -> Ident {
    let name_str = match name {
        "" => format!("p{}", index),
        n => n.to_snake_case(),
    };
    util::safe_ident(&name_str)
}

fn expand_input_name(index: usize, name: &str) -> TokenStream {
    let name = input_name_to_ident(index, name);
    quote! { #name }
}

pub(crate) fn expand_inputs_call_arg(inputs: &[Param]) -> TokenStream {
    let names = inputs
        .iter()
        .enumerate()
        .map(|(i, param)| expand_input_name(i, &param.name));
    quote! { ( #( #names ,)* ) }
}

fn expand_fn_outputs(outputs: &[Param]) -> Result<TokenStream> {
    match outputs.len() {
        0 => Ok(quote! { () }),
        1 => types::expand(&outputs[0].kind),
        _ => {
            let types = outputs
                .iter()
                .map(|param| types::expand(&param.kind))
                .collect::<Result<Vec<_>>>()?;
            Ok(quote! { (#( #types ),*) })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethcontract_common::abi::ParamType;

    #[test]
    fn input_name_to_ident_empty() {
        assert_eq!(input_name_to_ident(0, ""), util::ident("p0"));
    }

    #[test]
    fn input_name_to_ident_keyword() {
        assert_eq!(input_name_to_ident(0, "self"), util::ident("self_"));
    }

    #[test]
    fn input_name_to_ident_snake_case() {
        assert_eq!(
            input_name_to_ident(0, "CamelCase1"),
            util::ident("camel_case_1")
        );
    }

    #[test]
    fn expand_inputs_empty() {
        assert_quote!(expand_inputs(&[]).unwrap().to_string(), {},);
    }

    #[test]
    fn expand_inputs_() {
        assert_quote!(
            expand_inputs(

                &[
                    Param {
                        name: "a".to_string(),
                        kind: ParamType::Bool,
                    },
                    Param {
                        name: "b".to_string(),
                        kind: ParamType::Address,
                    },
                ],
            )
            .unwrap(),
            { , a: bool, b: self::ethcontract::Address },
        );
    }

    #[test]
    fn expand_fn_outputs_empty() {
        assert_quote!(expand_fn_outputs(&[],).unwrap(), { () });
    }

    #[test]
    fn expand_fn_outputs_single() {
        assert_quote!(
            expand_fn_outputs(&[Param {
                name: "a".to_string(),
                kind: ParamType::Bool,
            }])
            .unwrap(),
            { bool },
        );
    }

    #[test]
    fn expand_fn_outputs_muliple() {
        assert_quote!(
            expand_fn_outputs(&[
                Param {
                    name: "a".to_string(),
                    kind: ParamType::Bool,
                },
                Param {
                    name: "b".to_string(),
                    kind: ParamType::Address,
                },
            ],)
            .unwrap(),
            { (bool, self::ethcontract::Address) },
        );
    }
}
