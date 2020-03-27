use crate::contract::{types, Context};
use crate::util;
use anyhow::Result;
use ethcontract_common::abi::{Event, EventParam, Hash};
use ethcontract_common::abiext::EventExt;
use inflector::Inflector;
use proc_macro2::{Literal, TokenStream};
use quote::quote;

pub(crate) fn expand(cx: &Context) -> Result<TokenStream> {
    let structs_mod = expand_structs_mod(cx)?;
    let filters = expand_filters(cx)?;
    let all_events = expand_all_events(cx);

    Ok(quote! {
        #structs_mod
        #filters
        #all_events
    })
}

/// Expands into a module containing all the event data structures from the ABI.
fn expand_structs_mod(cx: &Context) -> Result<TokenStream> {
    let data_types = cx
        .artifact
        .abi
        .events()
        .map(|event| expand_data_type(event))
        .collect::<Result<Vec<_>>>()?;
    if data_types.is_empty() {
        return Ok(quote! {});
    }

    Ok(quote! {
        /// Module containing all generated data models for this contract's
        /// events.
        pub mod event_data {
            use super::ethcontract;

            #( #data_types )*
        }
    })
}

/// Expands an ABI event into a single event data type. This can expand either
/// into a structure or a tuple in the case where all event parameters (topics
/// and data) are anonymous.
fn expand_data_type(event: &Event) -> Result<TokenStream> {
    let event_name = expand_struct_name(event);

    let signature = expand_hash(event.signature());

    let abi_signature = event.abi_signature();
    let abi_signature_lit = Literal::string(&abi_signature);
    let abi_signature_doc = util::expand_doc(&format!("`{}`", abi_signature));

    let params = expand_params(event)?;

    let all_anonymous_fields = event.inputs.iter().all(|input| input.name.is_empty());
    let (data_type_definition, data_type_construction) = if all_anonymous_fields {
        expand_data_tuple(&event_name, &params)
    } else {
        expand_data_struct(&event_name, &params)
    };

    let params_len = Literal::usize_unsuffixed(params.len());
    let read_param_token = params
        .iter()
        .map(|(name, ty)| {
            quote! {
                let #name = #ty::from_token(tokens.next().unwrap())?;
            }
        })
        .collect::<Vec<_>>();

    Ok(quote! {
        #[derive(Clone, Debug, Default, Eq, PartialEq)]
        pub #data_type_definition

        impl #event_name {
            /// Retrieves the signature for the event this data corresponds to.
            /// This signature is the Keccak-256 hash of the ABI signature of
            /// this event.
            pub fn signature() -> self::ethcontract::H256 {
                #signature
            }

            /// Retrieves the ABI signature for the event this data corresponds
            /// to. For this event the value should always be:
            ///
            #abi_signature_doc
            pub fn abi_signature() -> &'static str {
                #abi_signature_lit
            }
        }

        impl self::ethcontract::web3::contract::tokens::Detokenize for #event_name {
            fn from_tokens(
                tokens: Vec<self::ethcontract::private::ethabi_9_0::Token>,
            ) -> Result<Self, self::ethcontract::web3::contract::Error> {
                use self::ethcontract::web3::contract::tokens::Tokenizable;

                if tokens.len() != #params_len {
                    return Err(self::ethcontract::web3::contract::Error::InvalidOutputType(format!(
                        "Expected {} tokens, got {}: {:?}",
                        #params_len,
                        tokens.len(),
                        tokens
                    )));
                }

                #[allow(unused_mut)]
                let mut tokens = tokens.into_iter();
                #( #read_param_token )*

                Ok(#data_type_construction)
            }
        }
    })
}

/// Expands an ABI event into an identifier for its event data type.
fn expand_struct_name(event: &Event) -> TokenStream {
    let event_name = util::ident(&event.name.to_pascal_case());
    quote! { #event_name }
}

/// Expands an ABI event into name-type pairs for each of its parameters.
fn expand_params(event: &Event) -> Result<Vec<(TokenStream, TokenStream)>> {
    event
        .inputs
        .iter()
        .enumerate()
        .map(|(i, input)| {
            // NOTE: Events can contain nameless values.
            let name = util::expand_input_name(i, &input.name);
            let ty = types::expand(&input.kind)?;
            Ok((name, ty))
        })
        .collect()
}

/// Expands an event data structure from its name-type parameter pairs. Returns
/// a tuple with the type definition (i.e. the struct declaration) and
/// construction (i.e. code for creating an instance of the event data).
fn expand_data_struct(
    name: &TokenStream,
    params: &[(TokenStream, TokenStream)],
) -> (TokenStream, TokenStream) {
    let fields = params
        .iter()
        .map(|(name, ty)| quote! { pub #name: #ty })
        .collect::<Vec<_>>();

    let param_names = params
        .iter()
        .map(|(name, _)| name)
        .cloned()
        .collect::<Vec<_>>();

    let definition = quote! { struct #name { #( #fields, )* } };
    let construction = quote! { #name { #( #param_names ),* } };

    (definition, construction)
}

/// Expands an event data named tuple from its name-type parameter pairs.
/// Returns a tuple with the type definition and construction.
fn expand_data_tuple(
    name: &TokenStream,
    params: &[(TokenStream, TokenStream)],
) -> (TokenStream, TokenStream) {
    let fields = params
        .iter()
        .map(|(_, ty)| quote! { pub #ty })
        .collect::<Vec<_>>();

    let param_names = params
        .iter()
        .map(|(name, _)| name)
        .cloned()
        .collect::<Vec<_>>();

    let definition = quote! { struct #name( #( #fields ),* ); };
    let construction = quote! { #name( #( #param_names ),* ) };

    (definition, construction)
}

/// Expands into an `Events` type with method definitions for creating event
/// streams for all non-anonymous contract events in the ABI.
fn expand_filters(cx: &Context) -> Result<TokenStream> {
    let standard_events = cx
        .artifact
        .abi
        .events()
        .filter(|event| !event.anonymous)
        .collect::<Vec<_>>();
    if standard_events.is_empty() {
        return Ok(quote! {});
    }

    let filters = standard_events
        .iter()
        .map(|event| expand_filter(event))
        .collect::<Result<Vec<_>>>()?;
    let builders = standard_events
        .iter()
        .map(|event| expand_builder_type(event))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        impl Contract {
            /// Retrieves a handle to a type containing for creating event
            /// streams for all the contract events.
            pub fn events(&self) -> Events<'_> {
                Events {
                    instance: self.raw_instance(),
                }
            }
        }

        pub struct Events<'a> {
            instance: &'a self::ethcontract::dyns::DynInstance,
        }

        impl Events<'_> {
            #( #filters )*
        }

        /// Module containing the generated event stream builders with type safe
        /// filter methods for this contract's events.
        pub mod event_builders {
            use super::ethcontract;
            use super::event_data;

            #( #builders )*
        }
    })
}

/// Expands into a single method for contracting an event stream.
fn expand_filter(event: &Event) -> Result<TokenStream> {
    let name = util::safe_ident(&event.name.to_snake_case());
    let builder_name = expand_builder_name(event);
    let signature = expand_hash(event.signature());

    Ok(quote! {
        /// Generated by `ethcontract`.
        pub fn #name(&self) -> self::event_builders::#builder_name {
            self::event_builders::#builder_name(
                self.instance.event(#signature)
                    .expect("generated event filter"),
            )
        }
    })
}

/// Expands an ABI event into a wrapped `EventBuilder` type with type-safe
/// filter methods.
fn expand_builder_type(event: &Event) -> Result<TokenStream> {
    let event_name = expand_struct_name(event);
    let builder_doc = util::expand_doc(&format!(
        "A builder for creating a filtered stream of `{}` events.",
        event_name
    ));
    let builder_name = expand_builder_name(event);
    let topic_filters = expand_builder_topic_filters(event)?;

    Ok(quote! {
        #builder_doc
        pub struct #builder_name(
            /// The inner event builder.
            pub self::ethcontract::dyns::DynEventBuilder<self::event_data::#event_name>,
        );

        impl #builder_name {
            /// Sets the starting block from which to stream logs for.
            ///
            /// If left unset defaults to the latest block.
            #[allow(clippy::wrong_self_convention)]
            pub fn from_block(mut self, block: self::ethcontract::BlockNumber) -> Self {
                self.0 = (self.0).from_block(block);
                self
            }

            /// Sets the last block from which to stream logs for.
            ///
            /// If left unset defaults to the streaming until the end of days.
            #[allow(clippy::wrong_self_convention)]
            pub fn to_block(mut self, block: self::ethcontract::BlockNumber) -> Self {
                self.0 = (self.0).to_block(block);
                self
            }

            /// The polling interval. This is used as the interval between consecutive
            /// `eth_getFilterChanges` calls to get filter updates.
            pub fn poll_interval(mut self, value: std::time::Duration) -> Self {
                self.0 = (self.0).poll_interval(value);
                self
            }

            #topic_filters

            /// Creates an event stream from the current event builder.
            pub fn stream(self) -> self::ethcontract::contract::EventStream<
                self::ethcontract::dyns::DynTransport,
                self::event_data::#event_name,
            > {
                (self.0).stream().expect("generated event topic filters")
            }
        }
    })
}

/// Expands an ABI event into filter methods for its indexed parameters.
fn expand_builder_topic_filters(event: &Event) -> Result<TokenStream> {
    let topic_filters = event
        .inputs
        .iter()
        .filter(|input| input.indexed)
        .enumerate()
        .map(|(topic_index, input)| expand_builder_topic_filter(topic_index, input))
        .collect::<Result<Vec<_>>>()?;

    Ok(quote! {
        #( #topic_filters )*
    })
}

/// Expands a event parameter into an event builder filter method for the
/// specified topic index.
fn expand_builder_topic_filter(topic_index: usize, param: &EventParam) -> Result<TokenStream> {
    let doc = util::expand_doc(&format!(
        "Adds a filter for the {} event parameter.",
        param.name,
    ));
    let topic = util::ident(&format!("topic{}", topic_index));
    let name = if param.name.is_empty() {
        topic.clone()
    } else {
        util::safe_ident(&param.name)
    };
    let ty = types::expand(&param.kind)?;

    Ok(quote! {
        #doc
        pub fn #name(mut self, topic: self::ethcontract::Topic<#ty>) -> Self {
            self.0 = (self.0).#topic(topic);
            self
        }
    })
}

/// Expands an ABI event into an identifier for its event data type.
fn expand_builder_name(event: &Event) -> TokenStream {
    let builder_name = util::ident(&format!("{}Builder", &event.name.to_pascal_case()));
    quote! { #builder_name }
}

/// Expands into the `all_events` method on the root contract type if it
/// contains events. Expands to nothing otherwise.
fn expand_all_events(cx: &Context) -> TokenStream {
    if cx.artifact.abi.events.is_empty() {
        return quote! {};
    }

    let event_enum = expand_event_enum(cx);
    let event_from_log = expand_event_from_log(cx);

    quote! {
        impl Contract {
            /// Returns a log stream with all events.
            pub fn all_events(&self) -> self::ethcontract::dyns::DynAllEventsBuilder<Event> {
                self::ethcontract::dyns::DynAllEventsBuilder::new(
                    self.raw_instance().web3(),
                    self.address(),
                )
            }
        }

        #event_enum
        #event_from_log
    }
}

/// Expands into an enum with one variant for each distinct event type,
/// including anonymous types.
fn expand_event_enum(cx: &Context) -> TokenStream {
    let variants = cx
        .artifact
        .abi
        .events()
        .map(|event| {
            let struct_name = expand_struct_name(&event);
            quote! {
                #struct_name(self::events::#struct_name)
            }
        })
        .collect::<Vec<_>>();

    quote! {
        /// A contract event.
        pub enum Event {
            #( #variants, )*
        }
    }
}

/// Expands the `ParseLog` implementation for the event enum.
fn expand_event_from_log(cx: &Context) -> TokenStream {
    let all_events = {
        let mut all_events = cx
            .artifact
            .abi
            .events()
            .map(|event| {
                let struct_name = expand_struct_name(&event);

                let name = Literal::string(&event.name);
                let decode_event = quote! {
                    log.decode(
                        &Contract::artifact()
                            .abi
                            .event(#name)
                            .expect("generated event decode")
                    )
                };

                (event, struct_name, decode_event)
            })
            .collect::<Vec<_>>();

        // NOTE: We sort the events by name so that the anonymous error decoding
        //   is consistent. Since the events are stored in a `HashMap`, there is
        //   no guaranteed order, and in the case where there is ambiguity in
        //   decoding anonymous events, its nice if they follow some strict and
        //   predictable order.
        all_events.sort_unstable_by_key(|(event, _, _)| &event.name);
        all_events
    };

    let standard_event_match_arms = all_events
        .iter()
        .filter(|(event, _, _)| !event.anonymous)
        .map(|(event, struct_name, decode_event)| {
            // These are all possible stardard (i.e. non-anonymous) events that
            // the contract can produce, along with its signature and index in
            // the contract ABI. For these, we match topic 0 to the signature
            // and try to decode.

            let signature = expand_hash(event.signature());
            quote! {
                #signature => Ok(Event::#struct_name(#decode_event?)),
            }
        })
        .collect::<Vec<_>>();

    let anonymous_event_try_decode = all_events
        .iter()
        .filter(|(event, _, _)| event.anonymous)
        .map(|(_, struct_name, decode_event)| {
            // For anonymous events, just try to decode one at a time and return
            // the first that succeeds.

            quote! {
                if let Ok(data) = #decode_event {
                    return Ok(Event::#struct_name(data));
                }
            }
        })
        .collect::<Vec<_>>();

    let invalid_data = quote! {
        Err(self::ethcontract::errors::ExecutionError::from(
            self::ethcontract::common::abi::Error::InvalidData
        ))
    };

    quote! {
        impl self::ethcontract::contract::ParseLog for Event {
            fn parse_log(
                log: self::ethcontract::RawLog,
            ) -> Result<Self, self::ethcontract::errors::ExecutionError> {
                let standard_event = log.topics
                    .get(0)
                    .copied()
                    .map(|topic| match topic {
                        #( #standard_event_match_arms )*
                        _ => #invalid_data,
                    });

                if let Some(Ok(data)) = standard_event {
                    return Ok(data);
                }

                #( #anonymous_event_try_decode )*

                #invalid_data
            }
        }
    }
}

/// Expands a 256-bit `Hash` into a literal representation that can be used with
/// quasi-quoting for code generation.
fn expand_hash(hash: Hash) -> TokenStream {
    let bytes = hash.as_bytes().iter().copied().map(Literal::u8_unsuffixed);

    quote! {
        self::ethcontract::H256([#( #bytes ),*])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ethcontract_common::abi::{EventParam, ParamType};

    #[test]
    fn expand_empty_filters() {
        assert_quote!(expand_filters(&Context::default()).unwrap(), {});
    }

    #[test]
    fn expand_transfer_filter() {
        let event = Event {
            name: "Transfer".into(),
            inputs: vec![
                EventParam {
                    name: "from".into(),
                    kind: ParamType::Address,
                    indexed: true,
                },
                EventParam {
                    name: "to".into(),
                    kind: ParamType::Address,
                    indexed: true,
                },
                EventParam {
                    name: "amount".into(),
                    kind: ParamType::Uint(256),
                    indexed: false,
                },
            ],
            anonymous: false,
        };
        let signature = expand_hash(event.signature());

        assert_quote!(expand_filter(&event).unwrap(), {
            /// Generated by `ethcontract`.
            pub fn transfer(&self) -> self::event_builders::TransferBuilder {
                self::event_builders::TransferBuilder(
                    self.instance.event(#signature)
                        .expect("generated event filter"),
                )
            }
        });
    }

    #[test]
    fn expand_transfer_builder_topic_filters() {
        let event = Event {
            name: "Transfer".into(),
            inputs: vec![
                EventParam {
                    name: "from".into(),
                    kind: ParamType::Address,
                    indexed: true,
                },
                EventParam {
                    name: "to".into(),
                    kind: ParamType::Address,
                    indexed: true,
                },
                EventParam {
                    name: "amount".into(),
                    kind: ParamType::Uint(256),
                    indexed: false,
                },
            ],
            anonymous: false,
        };

        #[rustfmt::skip]
        assert_quote!(expand_builder_topic_filters(&event).unwrap(), {
            #[doc = "Adds a filter for the from event parameter."]
            pub fn from(mut self, topic: self::ethcontract::Topic<self::ethcontract::Address>) -> Self {
                self.0 = (self.0).topic0(topic);
                self
            }

            #[doc = "Adds a filter for the to event parameter."]
            pub fn to(mut self, topic: self::ethcontract::Topic<self::ethcontract::Address>) -> Self {
                self.0 = (self.0).topic1(topic);
                self
            }
        });
    }

    #[test]
    fn expand_data_struct_value() {
        let event = Event {
            name: "Foo".into(),
            inputs: vec![
                EventParam {
                    name: "a".into(),
                    kind: ParamType::Bool,
                    indexed: false,
                },
                EventParam {
                    name: String::new(),
                    kind: ParamType::Address,
                    indexed: false,
                },
            ],
            anonymous: false,
        };

        let name = expand_struct_name(&event);
        let params = expand_params(&event).unwrap();
        let (definition, construction) = expand_data_struct(&name, &params);

        assert_quote!(definition, {
            struct Foo {
                pub a: bool,
                pub p1: self::ethcontract::Address,
            }
        });
        assert_quote!(construction, { Foo { a, p1 } });
    }

    #[test]
    fn expand_data_tuple_value() {
        let event = Event {
            name: "Foo".into(),
            inputs: vec![
                EventParam {
                    name: String::new(),
                    kind: ParamType::Bool,
                    indexed: false,
                },
                EventParam {
                    name: String::new(),
                    kind: ParamType::Address,
                    indexed: false,
                },
            ],
            anonymous: false,
        };

        let name = expand_struct_name(&event);
        let params = expand_params(&event).unwrap();
        let (definition, construction) = expand_data_tuple(&name, &params);

        assert_quote!(definition, {
            struct Foo(pub bool, pub self::ethcontract::Address);
        });
        assert_quote!(construction, { Foo(p0, p1) });
    }

    #[test]
    #[rustfmt::skip]
    fn expand_hash_value() {
        assert_quote!(
            expand_hash(
                "000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f".parse().unwrap()
            ),
            {
                self::ethcontract::H256::from([
                    0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15,
                    16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31
                ])
            },
        );
    }
}
