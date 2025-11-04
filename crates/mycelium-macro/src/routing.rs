//! Compile-time routing macro implementation
//!
//! Generates plain structs with direct function call routing to eliminate
//! Arc allocation and channel overhead (~65ns → ~2-3ns per handler).

use heck::ToSnakeCase;
use proc_macro2::{Span, TokenStream};
use quote::quote;
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
    for route in &config.routes {
        for handler in &route.handlers {
            if !all_handlers
                .iter()
                .any(|h| quote!(#h).to_string() == quote!(#handler).to_string())
            {
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
    let type_str = quote!(#ty).to_string();
    let snake_case = type_str.to_snake_case();
    Ident::new(&snake_case, Span::call_site())
}

/// Convert a message type to a method name (e.g., V2Swap -> route_v2_swap)
fn type_to_method_name(ty: &Type) -> Ident {
    let type_str = quote!(#ty).to_string();
    let snake_case = type_str.to_snake_case();
    let method_name = format!("route_{}", snake_case);
    Ident::new(&method_name, Span::call_site())
}

#[cfg(test)]
mod tests {
    use super::*;
    use quote::quote;
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
}
