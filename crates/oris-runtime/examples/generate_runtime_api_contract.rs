#[cfg(feature = "execution-server")]
use std::env;
#[cfg(feature = "execution-server")]
use std::path::PathBuf;

#[cfg(feature = "execution-server")]
use oris_runtime::execution_runtime::{
    canonical_runtime_api_contract_path, write_runtime_api_contract, RUNTIME_API_CONTRACT_DOC_PATH,
};

#[cfg(feature = "execution-server")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(canonical_runtime_api_contract_path);
    write_runtime_api_contract(&output_path)?;
    eprintln!(
        "Wrote runtime API contract to {} (canonical: {}).",
        output_path.display(),
        RUNTIME_API_CONTRACT_DOC_PATH,
    );
    Ok(())
}

#[cfg(not(feature = "execution-server"))]
fn main() {
    eprintln!("generate_runtime_api_contract example requires feature: execution-server");
}
