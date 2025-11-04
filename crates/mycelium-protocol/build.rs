// Build script for mycelium-protocol
// Note: codegen is now in src/codegen.rs as a public module
// This build.rs keeps its own inline copy to avoid circular dependencies

fn main() {
    println!("cargo:rerun-if-changed=contracts.yaml");

    // Generate protocol messages from contracts.yaml
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let output_path = format!("{}/generated_messages.rs", out_dir);

    // We include the codegen module directly here
    // Applications will use mycelium_protocol::codegen::generate_from_yaml()
    #[path = "src/codegen.rs"]
    mod codegen;

    codegen::generate_from_yaml("contracts.yaml", &output_path)
        .expect("Failed to generate protocol messages from contracts.yaml");
}
