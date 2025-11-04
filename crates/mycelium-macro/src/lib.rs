//! Proc macros for Mycelium services and compile-time routing

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, ImplItem, ItemImpl};

mod routing;

#[proc_macro_attribute]
pub fn service(_attr: TokenStream, item: TokenStream) -> TokenStream {
    let mut input = parse_macro_input!(item as ItemImpl);

    // Extract the struct name
    let struct_name = &input.self_ty;
    let struct_name_str = quote!(#struct_name).to_string();

    // Find and rename the user's methods to avoid conflicts
    let mut has_run = false;
    let mut has_startup = false;
    let mut has_shutdown = false;

    for item in &mut input.items {
        if let ImplItem::Fn(method) = item {
            let method_name = method.sig.ident.to_string();
            match method_name.as_str() {
                "run" => {
                    has_run = true;
                    // Rename to __service_run to avoid conflict
                    method.sig.ident = syn::Ident::new("__service_run", method.sig.ident.span());
                }
                "startup" => {
                    has_startup = true;
                    method.sig.ident =
                        syn::Ident::new("__service_startup", method.sig.ident.span());
                }
                "shutdown" => {
                    has_shutdown = true;
                    method.sig.ident =
                        syn::Ident::new("__service_shutdown", method.sig.ident.span());
                }
                _ => {}
            }
        }
    }

    if !has_run {
        panic!("#[service] requires a run() method");
    }

    // Generate the Service trait implementation based on what methods exist
    let startup_impl = if has_startup {
        quote! {
            fn startup(&mut self)
                -> impl std::future::Future<Output = anyhow::Result<()>> + Send
            {
                self.__service_startup()
            }
        }
    } else {
        quote! {}
    };

    let shutdown_impl = if has_shutdown {
        quote! {
            fn shutdown(&mut self)
                -> impl std::future::Future<Output = anyhow::Result<()>> + Send
            {
                self.__service_shutdown()
            }
        }
    } else {
        quote! {}
    };

    let expanded = quote! {
        // Keep the modified impl block (with renamed methods)
        #input

        // Generate Service trait impl that delegates to renamed methods
        impl ::mycelium_transport::Service for #struct_name {
            const NAME: &'static str = #struct_name_str;

            fn run(&mut self, ctx: &::mycelium_transport::ServiceContext)
                -> impl std::future::Future<Output = anyhow::Result<()>> + Send
            {
                self.__service_run(ctx)
            }

            #startup_impl

            #shutdown_impl
        }
    };

    TokenStream::from(expanded)
}

/// Compile-time routing macro for ultra-low latency message handling
///
/// Generates a plain struct with direct function call routing, reducing
/// overhead from ~65ns (Arc-based MessageBus) to ~2-3ns per handler.
///
/// # Example
///
/// ```ignore
/// use mycelium_transport::MessageHandler;
/// use mycelium_macro::routing_config;
///
/// struct RiskManager;
/// impl MessageHandler<V2Swap> for RiskManager {
///     fn handle(&mut self, swap: &V2Swap) {
///         // Handle swap
///     }
/// }
///
/// struct ArbitrageDetector;
/// impl MessageHandler<V2Swap> for ArbitrageDetector {
///     fn handle(&mut self, swap: &V2Swap) {
///         // Detect arbitrage
///     }
/// }
///
/// // Generate routing struct
/// routing_config! {
///     name: TradingServices,
///     routes: {
///         V2Swap => [RiskManager, ArbitrageDetector],
///     }
/// }
///
/// // Use it
/// let mut services = TradingServices::new(
///     RiskManager,
///     ArbitrageDetector,
/// );
/// services.route_v2_swap(&swap);  // 2-3ns overhead
/// ```
#[proc_macro]
pub fn routing_config(input: TokenStream) -> TokenStream {
    let config = parse_macro_input!(input as routing::RoutingConfig);
    let output = routing::generate_routing_struct(config);
    TokenStream::from(output)
}
