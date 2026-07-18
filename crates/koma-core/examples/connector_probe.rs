use std::{env, fs, process::ExitCode};

use koma_core::{ConnectorManifest, DeclarativeImporter, LinkImporter};

#[tokio::main]
async fn main() -> ExitCode {
    let mut arguments = env::args().skip(1);
    let Some(package_path) = arguments.next() else {
        eprintln!("usage: connector_probe <connector.koma-connector.json> <source-url>");
        return ExitCode::FAILURE;
    };
    let Some(source) = arguments.next() else {
        eprintln!("usage: connector_probe <connector.koma-connector.json> <source-url>");
        return ExitCode::FAILURE;
    };
    let result = async {
        let package = fs::read(package_path)?;
        let manifest = ConnectorManifest::from_json(&package)?;
        let importer = DeclarativeImporter::new(manifest)?;
        let preview = importer.preview(&source).await?;
        println!(
            "{}",
            serde_json::to_string_pretty(&preview)
                .map_err(|error| koma_core::KomaError::Other(error.to_string()))?
        );
        Ok::<_, koma_core::KomaError>(())
    }
    .await;
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("connector probe failed: {error}");
            ExitCode::FAILURE
        }
    }
}
