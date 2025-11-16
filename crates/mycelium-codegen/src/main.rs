use std::path::PathBuf;

use clap::Parser;
use mycelium_protocol::codegen::{
    generate_from_yaml, generate_from_yaml_external, generate_ocaml_from_yaml,
    generate_python_from_yaml, CodegenError,
};

/// CLI helper for generating Mycelium protocol bindings.
#[derive(Debug, Parser)]
#[command(
    author,
    version,
    about = "Generate Rust, Python, or OCaml bindings from contracts.yaml"
)]
struct Args {
    /// Path to contracts.yaml
    #[arg(long, default_value = "contracts.yaml")]
    contracts: PathBuf,

    /// Output path for generated Rust code
    #[arg(long)]
    rust_out: Option<PathBuf>,

    /// Output path for generated Python module
    #[arg(long)]
    python_out: Option<PathBuf>,

    /// Output path for generated OCaml module
    #[arg(long)]
    ocaml_out: Option<PathBuf>,

    /// Use external imports for Rust output (for downstream crates)
    #[arg(long, default_value_t = false)]
    external_imports: bool,
}

fn main() -> Result<(), CodegenError> {
    let args = Args::parse();
    if args.rust_out.is_none() && args.python_out.is_none() && args.ocaml_out.is_none() {
        eprintln!("error: specify --rust-out, --python-out, and/or --ocaml-out");
        std::process::exit(1);
    }

    if let Some(path) = args.rust_out {
        if args.external_imports {
            generate_from_yaml_external(&args.contracts, &path)?;
        } else {
            generate_from_yaml(&args.contracts, &path)?;
        }
    }

    if let Some(path) = args.python_out {
        generate_python_from_yaml(&args.contracts, &path)?;
    }

    if let Some(path) = args.ocaml_out {
        generate_ocaml_from_yaml(&args.contracts, &path)?;
    }

    Ok(())
}
