use bifti::{Catalog, Phantom, Registry};
use rand::seq::IndexedRandom;
use tracing_chrome::ChromeLayerBuilder;
use tracing_subscriber::{EnvFilter, prelude::*};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // console: human-readable span timings. chrome file: open in
    // https://ui.perfetto.dev for a timeline view of the same spans.
    // only bifti's own spans - deps like ureq/rustls trace their internals too.
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("bifti=debug,warn"));
    let (chrome_layer, _chrome_guard) = ChromeLayerBuilder::new().file("trace.json").build();
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_span_events(tracing_subscriber::fmt::format::FmtSpan::CLOSE),
        )
        .with(chrome_layer)
        .init();

    let registry = Registry::load()?;
    let catalog = Catalog::load()?;
    let mut rng = rand::rng();

    // Pick from the catalog (the discovery list), then resolve to the immutable
    // registry entry.
    let (label, collection) = catalog
        .iter()
        .collect::<Vec<_>>()
        .choose(&mut rng)
        .copied()
        .expect("catalog has at least one entry");
    let entry = registry
        .get(collection)
        .unwrap_or_else(|| panic!("catalog entry {label:?} -> {collection:?} is not in the registry"));
    let files = entry.phantom_files();
    let phantom_name = files
        .choose(&mut rng)
        .copied()
        .expect("collection has at least one phantom");

    println!("downloading {label:?} -> {collection}/{phantom_name}...");
    let cache_dir = std::path::Path::new("cache");
    let json_path = registry.load_registry_phantom(collection, phantom_name, cache_dir)?;

    println!("loading {}...", json_path.display());
    let phantom = Phantom::load(&json_path)?;

    println!("loaded phantom with {} tissue(s):", phantom.tissues.len());
    for name in phantom.tissues.keys() {
        println!("  {name}");
    }
    Ok(())
}
