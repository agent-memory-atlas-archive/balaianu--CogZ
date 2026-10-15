//! One-shot normalization: re-embed every code entity in a `.cogz` DB
//! through the canonical solo path. Surgical alternative to
//! `reset`+`index` — leaves entities, telemetry, and knowledge
//! vectors untouched.
//!
//! Usage: `cargo run --release --example reembed_code_entities -- [db-path]`
//! Defaults to `.cogz/cogz.db` (or `$COGZ_REEMBED_DB`).

use cogz::embed::model_type::ModelType;
use cogz::embed::onnx::OnnxEmbeddingModel;
use cogz::files::embed_sync::{embed_entities, store_embeddings};
use cogz::storage::crud::Entity;

fn main() -> anyhow::Result<()> {
    let model = OnnxEmbeddingModel::new(ModelType::Code, &cogz::embed::models_dir(), 768);
    anyhow::ensure!(
        model.model_files_exist(),
        "code model not found — run `cogz index` first to download it"
    );

    let db = std::env::args()
        .nth(1)
        .or_else(|| std::env::var("COGZ_REEMBED_DB").ok())
        .unwrap_or_else(|| ".cogz/cogz.db".to_string());
    // Storage::open registers the sqlite-vec extension — the vec0
    // embedding tables don't exist on a plain rusqlite connection.
    let storage = cogz::storage::Storage::open(std::path::Path::new(&db), 768)?;

    let entities: Vec<Entity> = {
        let conn = storage.conn();
        let mut stmt = conn.prepare(
            "SELECT id, type, title, content FROM entities \
             WHERE type IN ('function','file','class','module','method') \
             AND status != 'pruned'",
        )?;
        stmt.query_map([], |r| {
            let mut e = Entity::new(
                &r.get::<_, String>(0)?,
                &r.get::<_, String>(1)?,
                "",
                &r.get::<_, String>(3)?,
            );
            e.title = r.get::<_, Option<String>>(2)?;
            Ok(e)
        })?
        .collect::<Result<Vec<_>, _>>()?
    };
    eprintln!("re-embedding {} code entities solo", entities.len());

    let t0 = std::time::Instant::now();
    let cache = cogz::embed::EmbeddingCache::new();
    let embedded = embed_entities(&model, &cache, &entities);
    eprintln!(
        "embedded {}/{} in {:?} — storing",
        embedded.len(),
        entities.len(),
        t0.elapsed()
    );

    let mut conn = storage.conn();
    let stored = store_embeddings(&mut conn, &embedded);
    eprintln!("stored {}/{}", stored, entities.len());
    anyhow::ensure!(stored == entities.len(), "stored {} != {}", stored, entities.len());
    Ok(())
}
