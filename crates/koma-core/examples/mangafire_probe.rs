use std::{collections::BTreeSet, env, path::PathBuf};

use koma_core::{
    ImportEvent, ImportOptions, ImportScope, LinkImporter, MangaFireImporter, open_publication,
};
use serde::Serialize;
use serde_json::json;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct SampledPage {
    index: usize,
    mime_type: String,
    byte_size: usize,
    blake3: String,
}

fn usage() -> anyhow::Error {
    anyhow::anyhow!(
        "usage:\n  cargo run -p koma-core --example mangafire_probe -- preview <MangaFire URL>\n  cargo run -p koma-core --example mangafire_probe -- download <MangaFire URL> <destination directory>\n  cargo run -p koma-core --example mangafire_probe -- download-chapter <MangaFire URL> <chapter id> <destination directory>\n  cargo run -p koma-core --example mangafire_probe -- download-series <MangaFire URL> <destination directory>"
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let mut arguments = env::args().skip(1);
    let command = arguments.next().ok_or_else(usage)?;
    let source = arguments.next().ok_or_else(usage)?;
    let importer = MangaFireImporter::new()?;

    match command.as_str() {
        "preview" => {
            if arguments.next().is_some() {
                return Err(usage());
            }
            let preview = importer.preview(&source).await?;
            println!("{}", serde_json::to_string_pretty(&preview)?);
        }
        "download" | "download-chapter" | "download-series" => {
            let chapter_id = if command == "download-chapter" {
                Some(
                    arguments
                        .next()
                        .ok_or_else(usage)?
                        .parse::<u64>()
                        .map_err(|_| usage())?,
                )
            } else {
                None
            };
            let destination = PathBuf::from(arguments.next().ok_or_else(usage)?);
            if arguments.next().is_some() {
                return Err(usage());
            }
            let mut options = ImportOptions::new(&destination);
            match command.as_str() {
                "download-chapter" => {
                    options.scope = ImportScope::Chapter;
                    options.chapter_id = chapter_id;
                }
                "download-series" => options.scope = ImportScope::Series,
                _ => {}
            }

            let (event_sender, mut event_receiver) =
                tokio::sync::mpsc::unbounded_channel::<ImportEvent>();
            let event_task = tokio::spawn(async move {
                while let Some(event) = event_receiver.recv().await {
                    let should_report = match &event {
                        ImportEvent::Downloading { completed, total } => {
                            *completed == 1 || *completed == *total || *completed % 25 == 0
                        }
                        _ => true,
                    };
                    if should_report {
                        eprintln!("{}", serde_json::to_string(&event).unwrap_or_default());
                    }
                }
            });

            let receipt = importer
                .import(&source, &options, Some(&event_sender))
                .await?;
            drop(event_sender);
            event_task.await?;

            let reader = open_publication(&receipt.output_path, None)?;
            let manifest = reader.manifest().clone();
            anyhow::ensure!(
                manifest.pages.len() == receipt.page_count,
                "receipt recorded {} pages but the reopened CBZ exposed {}",
                receipt.page_count,
                manifest.pages.len()
            );

            let mut sample_indices = BTreeSet::from([
                0,
                manifest.pages.len() / 2,
                manifest.pages.len().saturating_sub(1),
            ]);
            if manifest.pages.is_empty() {
                sample_indices.clear();
            }
            let mut sampled_pages = Vec::new();
            for index in sample_indices {
                let page = reader.read_page(index)?;
                sampled_pages.push(SampledPage {
                    index,
                    mime_type: page.mime_type,
                    byte_size: page.bytes.len(),
                    blake3: blake3::hash(&page.bytes).to_hex().to_string(),
                });
            }

            let output_size = std::fs::metadata(&receipt.output_path)?.len();
            println!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "receipt": receipt,
                    "reopened": {
                        "format": manifest.format,
                        "pageCount": manifest.pages.len(),
                        "title": manifest.metadata.title,
                        "series": manifest.metadata.series,
                        "language": manifest.metadata.language,
                        "outputBytes": output_size,
                        "sampledPages": sampled_pages,
                    }
                }))?
            );
        }
        _ => return Err(usage()),
    }

    Ok(())
}
