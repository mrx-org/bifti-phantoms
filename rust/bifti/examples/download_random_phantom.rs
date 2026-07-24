use bifti::{Phantom, Registry};
use rand::seq::IndexedRandom;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let registry = Registry::load()?;
    let mut rng = rand::rng();

    let (collection, entry) = registry
        .iter()
        .collect::<Vec<_>>()
        .choose(&mut rng)
        .copied()
        .expect("registry has at least one collection");
    let phantom_name = entry
        .phantoms
        .choose(&mut rng)
        .expect("collection has at least one phantom");

    println!("downloading {collection}/{phantom_name}...");
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
