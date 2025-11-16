//! Compile-time routing macro implementation
//!
//! Generates plain structs with direct function call routing to eliminate
//! Arc allocation and channel overhead (~65ns → ~2-3ns per handler).

use heck::ToSnakeCase;
use proc_macro2::{Span, TokenStream};
use quote::quote;
use std::collections::HashSet;
use syn::{
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
    token::{Comma, FatArrow},
    Ident, Result, Token, Type,
};

/// Parse input for routing_config! macro
///
/// Syntax:
/// ```ignore
/// routing_config! {
///     name: TradingServices,
///     routes: {
///         V2Swap => [ArbitrageService, RiskManager],
///         Signal => [ExecutionService],
///     }
/// }
/// ```
pub struct RoutingConfig {
    pub struct_name: Ident,
    pub routes: Vec<Route>,
}

pub struct Route {
    pub message_type: Type,
    pub handlers: Vec<Type>,
}

impl Parse for RoutingConfig {
    fn parse(input: ParseStream) -> Result<Self> {
        // Parse "name: StructName,"
        let name_ident: Ident = input.parse()?;
        if name_ident != "name" {
            return Err(syn::Error::new(name_ident.span(), "expected 'name'"));
        }
        input.parse::<Token![:]>()?;
        let struct_name = input.parse::<Ident>()?;
        input.parse::<Token![,]>()?;

        // Parse "routes: { ... }"
        let routes_ident: Ident = input.parse()?;
        if routes_ident != "routes" {
            return Err(syn::Error::new(routes_ident.span(), "expected 'routes'"));
        }
        input.parse::<Token![:]>()?;

        let routes_content;
        syn::braced!(routes_content in input);

        // Parse routes: MessageType => [Handler1, Handler2], ...
        let mut routes = Vec::new();
        while !routes_content.is_empty() {
            let message_type = routes_content.parse::<Type>()?;
            routes_content.parse::<FatArrow>()?;

            let handlers_content;
            syn::bracketed!(handlers_content in routes_content);
            let handler_types: Punctuated<Type, Comma> =
                handlers_content.parse_terminated(Type::parse, Token![,])?;

            routes.push(Route {
                message_type,
                handlers: handler_types.into_iter().collect(),
            });

            // Optional trailing comma
            if !routes_content.is_empty() {
                routes_content.parse::<Token![,]>()?;
            }
        }

        Ok(RoutingConfig {
            struct_name,
            routes,
        })
    }
}

/// Generate the routing struct and implementation
pub fn generate_routing_struct(config: RoutingConfig) -> TokenStream {
    let struct_name = &config.struct_name;

    // Collect all unique handler types
    let mut all_handlers = Vec::new();
    let mut seen = HashSet::new();
    for route in &config.routes {
        for handler in &route.handlers {
            let key = normalized_type_name(handler);
            if seen.insert(key) {
                all_handlers.push(handler.clone());
            }
        }
    }

    // Generate struct fields (one field per unique handler)
    let fields = all_handlers.iter().map(|handler_type| {
        let field_name = type_to_field_name(handler_type);
        quote! {
            #field_name: #handler_type
        }
    });

    // Generate routing methods
    let routing_methods = config.routes.iter().map(|route| {
        let message_type = &route.message_type;
        let method_name = type_to_method_name(message_type);

        // Generate handler calls
        let handler_calls = route.handlers.iter().map(|handler_type| {
            let field_name = type_to_field_name(handler_type);
            quote! {
                ::mycelium_transport::MessageHandler::<#message_type>::handle(&mut self.#field_name, msg);
            }
        });

        quote! {
            #[inline(always)]
            pub fn #method_name(&mut self, msg: &#message_type) {
                #(#handler_calls)*
            }
        }
    });

    // Generate constructor parameters
    let constructor_params = all_handlers.iter().map(|handler_type| {
        let field_name = type_to_field_name(handler_type);
        quote! {
            #field_name: #handler_type
        }
    });

    // Generate constructor field assignments
    let constructor_fields = all_handlers.iter().map(|handler_type| {
        let field_name = type_to_field_name(handler_type);
        quote! {
            #field_name
        }
    });

    quote! {
        pub struct #struct_name {
            #(#fields),*
        }

        impl #struct_name {
            /// Create a new routing struct with all handlers
            pub fn new(#(#constructor_params),*) -> Self {
                Self {
                    #(#constructor_fields),*
                }
            }

            #(#routing_methods)*
        }
    }
}

/// Convert a type to a field name (e.g., ArbitrageService -> arbitrage_service)
fn type_to_field_name(ty: &Type) -> Ident {
    let snake_case = normalized_type_name(ty).to_snake_case();
    let cleaned = snake_case.trim_matches('_');
    let name = if cleaned.is_empty() {
        "handler"
    } else {
        cleaned
    };
    Ident::new(name, Span::call_site())
}

/// Convert a message type to a method name (e.g., V2Swap -> route_v2_swap)
fn type_to_method_name(ty: &Type) -> Ident {
    let type_name = normalized_type_name(ty);
    let snake_case = type_name.to_snake_case();
    let cleaned = snake_case.trim_matches('_');
    let method_name = if cleaned.is_empty() {
        "route_handler".to_string()
    } else {
        format!("route_{}", cleaned)
    };
    Ident::new(&method_name, Span::call_site())
}

fn normalized_type_name(ty: &Type) -> String {
    let type_str = quote!(#ty).to_string();
    let mut normalized = String::with_capacity(type_str.len());
    for ch in type_str.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch);
        } else {
            normalized.push('_');
        }
    }
    while normalized.contains("__") {
        normalized = normalized.replace("__", "_");
    }
    normalized.trim_matches('_').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use syn::parse_quote;

    #[test]
    fn test_type_to_field_name() {
        let ty: Type = parse_quote!(ArbitrageService);
        let field_name = type_to_field_name(&ty);
        assert_eq!(field_name.to_string(), "arbitrage_service");
    }

    #[test]
    fn test_type_to_method_name() {
        let ty: Type = parse_quote!(V2Swap);
        let method_name = type_to_method_name(&ty);
        assert_eq!(method_name.to_string(), "route_v2_swap");
    }

    #[test]
    fn test_type_to_field_name_with_path() {
        let ty: Type = parse_quote!(crate::adapters::RiskManager);
        let field_name = type_to_field_name(&ty);
        assert_eq!(field_name.to_string(), "crate_adapters_risk_manager");
    }

    #[test]
    fn test_type_to_method_name_with_generic() {
        let ty: Type = parse_quote!(ShardHandler<SwapMessage>);
        let method_name = type_to_method_name(&ty);
        assert_eq!(method_name.to_string(), "route_shard_handler_swap_message");
    }
}
