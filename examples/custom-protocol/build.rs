use mycelium_protocol::codegen::generate_from_yaml_external;

fn main() {
    // Rebuild if contracts.yaml changes
    println!("cargo:rerun-if-changed=contracts.yaml");

    // Generate protocol messages from your custom contracts.yaml
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let output_path = format!("{}/generated_messages.rs", out_dir);

    generate_from_yaml_external("contracts.yaml", &output_path)
        .expect("Failed to generate protocol messages from contracts.yaml");
}
