//! Batch-composition determinism measurement (item 57).
//!
//! `encode_batch` pads to the longest text in each batch, so a text's
//! embedding could depend on its batchmates. This test loads real ONNX
//! models to measure the drift across compositions and the wall-clock
//! cost of candidate deterministic strategies.
//!
//! Ignored by default — needs the real ONNX models and the dogfood DB.
//! Run: `cargo test --test test_embed_determinism -- --ignored --nocapture`

use cogz::embed::model::EmbeddingModel;
use cogz::embed::model_type::ModelType;
use cogz::embed::onnx::OnnxEmbeddingModel;
use rusqlite::Connection;

const BATCH: usize = 32;

fn model(mt: ModelType, dim: usize) -> Option<OnnxEmbeddingModel> {
    let base = cogz::embed::models_dir();
    let m = OnnxEmbeddingModel::new(mt, &base, dim);
    m.model_files_exist().then_some(m)
}

/// Real entity texts from the dogfood index — varied lengths.
fn corpus() -> Vec<String> {
    let conn = Connection::open(".cogz/cogz.db").unwrap();
    let mut stmt = conn
        .prepare("SELECT content FROM entities WHERE status = 'active' ORDER BY id LIMIT 256")
        .unwrap();
    stmt.query_map([], |r| r.get(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap()
}

fn cosine(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn drift_stats(solo: &[Vec<f32>], batched: &[Vec<f32>]) -> (f32, f32) {
    let mut min = f32::MAX;
    let mut sum = 0.0f32;
    for (s, b) in solo.iter().zip(batched) {
        let c = cosine(s, b);
        min = min.min(c);
        sum += c;
    }
    (min, sum / solo.len() as f32)
}

fn embed_solo(model: &OnnxEmbeddingModel, texts: &[&str]) -> Vec<Vec<f32>> {
    texts
        .iter()
        .map(|t| model.embed(std::slice::from_ref(t)).unwrap().remove(0))
        .collect()
}

fn embed_chunked(model: &OnnxEmbeddingModel, texts: &[&str]) -> Vec<Vec<f32>> {
    model.embed(texts).unwrap()
}

/// Sort by char length, group into equal-length buckets, chunk within
/// buckets, restore original order. Bucket key is derived per text so
/// padded_len depends only on the text's own bucket.
fn embed_bucketed(model: &OnnxEmbeddingModel, texts: &[&str]) -> Vec<Vec<f32>> {
    const BUCKET: usize = 256;
    let mut order: Vec<usize> = (0..texts.len()).collect();
    order.sort_by_key(|&i| texts[i].len() / BUCKET);

    let mut out = vec![Vec::new(); texts.len()];
    let mut i = 0;
    while i < order.len() {
        let bucket = texts[order[i]].len() / BUCKET;
        let mut group: Vec<usize> = Vec::new();
        while i < order.len() && texts[order[i]].len() / BUCKET == bucket {
            group.push(order[i]);
            i += 1;
        }
        for chunk in group.chunks(BATCH) {
            let chunk_texts: Vec<&str> = chunk.iter().map(|&j| texts[j]).collect();
            for (j, emb) in model.embed(&chunk_texts).unwrap().into_iter().enumerate() {
                out[chunk[j]] = emb;
            }
        }
    }
    out
}

/// Force max padding in every batch by injecting a long text per
/// chunk, then dropping its row — simulates "always pad to 512".
fn embed_pad_max(model: &OnnxEmbeddingModel, texts: &[&str]) -> Vec<Vec<f32>> {
    let filler = "x ".repeat(600); // tokenizes past the 512 cap
    let mut out = Vec::with_capacity(texts.len());
    for chunk in texts.chunks(BATCH - 1) {
        let mut batch: Vec<&str> = chunk.to_vec();
        batch.push(&filler);
        let embs = model.embed(&batch).unwrap();
        out.extend(embs.into_iter().take(chunk.len()));
    }
    out
}

#[test]
#[ignore = "needs real ONNX model + .cogz corpus"]
fn measure_batch_composition_drift() {
    let Some(model) = model(ModelType::Knowledge, 768) else {
        eprintln!("knowledge model not found — skipping");
        return;
    };
    let texts = corpus();
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

    let t0 = std::time::Instant::now();
    let solo = embed_solo(&model, &refs);
    let solo_t = t0.elapsed();

    let t0 = std::time::Instant::now();
    let chunked = embed_chunked(&model, &refs);
    let chunked_t = t0.elapsed();

    let t0 = std::time::Instant::now();
    let bucketed = embed_bucketed(&model, &refs);
    let bucketed_t = t0.elapsed();

    let t0 = std::time::Instant::now();
    let padded = embed_pad_max(&model, &refs);
    let padded_t = t0.elapsed();

    // Same corpus, different composition: reverse order re-chunks texts
    // into different batchmate sets — the live nondeterminism symptom.
    let mut rev = refs.clone();
    rev.reverse();
    let rev_embs = embed_chunked(&model, &rev);
    let mut chunked_rev = vec![Vec::new(); texts.len()];
    for (i, emb) in rev_embs.into_iter().enumerate() {
        chunked_rev[texts.len() - 1 - i] = emb;
    }

    let (min_c, avg_c) = drift_stats(&solo, &chunked);
    let (min_r, avg_r) = drift_stats(&solo, &chunked_rev);
    let (min_b, avg_b) = drift_stats(&solo, &bucketed);
    let (min_p, avg_p) = drift_stats(&solo, &padded);

    // Cross-composition determinism for bucketed grouping.
    let bucketed_rev = embed_bucketed(&model, &rev);
    let (min_bd, avg_bd) = drift_stats(&bucketed, &{
        let mut v = vec![Vec::new(); texts.len()];
        for (i, e) in bucketed_rev.into_iter().enumerate() {
            v[texts.len() - 1 - i] = e;
        }
        v
    });

    eprintln!("\n=== batch composition drift (256 texts, cosine vs solo) ===");
    eprintln!("strategy          wall_ms   min_cos  avg_cos");
    eprintln!(
        "chunked(fwd)      {:>7}   {:.4}   {:.4}",
        chunked_t.as_millis(),
        min_c,
        avg_c
    );
    eprintln!(
        "chunked(rev)      {:>7}   {:.4}   {:.4}",
        "(same)", min_r, avg_r
    );
    eprintln!(
        "bucketed          {:>7}   {:.4}   {:.4}",
        bucketed_t.as_millis(),
        min_b,
        avg_b
    );
    eprintln!(
        "pad-to-512        {:>7}   {:.4}   {:.4}",
        padded_t.as_millis(),
        min_p,
        avg_p
    );
    eprintln!("solo              {:>7}   (reference)", solo_t.as_millis());
    eprintln!(
        "\nbucketed cross-composition determinism: min_cos={:.6} avg_cos={:.6}",
        min_bd, avg_bd
    );
}

/// The extreme composition case: a short text embedded alone vs
/// buried among 500-token batchmates, on both models. If masking is
/// exact, drift should be ~zero for every model.
#[test]
#[ignore = "needs real ONNX models"]
fn extreme_length_mix_drift() {
    let short = "use early returns to reduce nesting";
    let shorts: Vec<String> = (0..32)
        .map(|i| format!("fn tiny_{i}() {{ id_{i}() }}"))
        .collect();
    let longs: Vec<String> = (0..8)
        .map(|i| {
            (0..400)
                .map(|j| format!("fn helper_{i}_{j}() {{ compute_{j}(); }}"))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .collect();

    for (mt, dim) in [(ModelType::Knowledge, 768), (ModelType::Code, 768)] {
        let Some(model) = model(mt, dim) else {
            eprintln!("{:?} model not found — skipping", mt);
            continue;
        };
        let solo = model.embed(&[short]).unwrap().remove(0);

        // Short text among eight long texts — max padding pressure.
        let mut batch: Vec<&str> = longs.iter().map(|s| s.as_str()).collect();
        batch.push(short);
        let mixed = model.embed(&batch).unwrap().remove(8);

        // Same short text in a homogeneous batch of short texts —
        // does length-grouping restore the solo vector?
        let mut hbatch: Vec<&str> = shorts.iter().take(8).map(|s| s.as_str()).collect();
        hbatch.push(short);
        let grouped = model.embed(&hbatch).unwrap().remove(8);

        eprintln!(
            "{:?}: solo-vs-mixed={:.6}  solo-vs-grouped={:.6}",
            mt,
            cosine(&solo, &mixed),
            cosine(&solo, &grouped)
        );

        // Drift vs batch size — is it the batch dimension itself?
        let mut sizes = String::new();
        for n in [2usize, 4, 16] {
            let mut b: Vec<&str> = shorts.iter().take(n - 1).map(|s| s.as_str()).collect();
            b.push(short);
            let v = model.embed(&b).unwrap().remove(n - 1);
            sizes.push_str(&format!(" b{n}={:.6}", cosine(&solo, &v)));
        }
        eprintln!("{:?}: solo-vs-batch-size:{sizes}", mt);
    }
}

/// Perf + determinism on real code entities (the drifting model's
/// actual workload). Requires code entities in the dogfood index.
#[test]
#[ignore = "needs real ONNX models + .cogz corpus"]
fn code_corpus_bucketed_vs_chunked() {
    let Some(model) = model(ModelType::Code, 768) else {
        eprintln!("code model not found — skipping");
        return;
    };
    let conn = Connection::open(".cogz/cogz.db").unwrap();
    let mut stmt = conn
        .prepare(
            "SELECT content FROM entities WHERE type IN ('function','file','class','module') \
             AND status='active' ORDER BY id LIMIT 384",
        )
        .unwrap();
    let texts = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<String>, _>>()
        .unwrap();
    eprintln!("code corpus: {} texts", texts.len());
    let refs: Vec<&str> = texts.iter().map(|s| s.as_str()).collect();

    let t0 = std::time::Instant::now();
    let solo = embed_solo(&model, &refs);
    let solo_t = t0.elapsed();

    let t0 = std::time::Instant::now();
    let chunked = embed_chunked(&model, &refs);
    let chunked_t = t0.elapsed();

    let t0 = std::time::Instant::now();
    let bucketed = embed_bucketed(&model, &refs);
    let bucketed_t = t0.elapsed();

    // Cross-composition: reverse corpus, same texts, different batches.
    let mut rev = refs.clone();
    rev.reverse();
    let mut chunked_rev = vec![Vec::new(); texts.len()];
    for (i, e) in embed_chunked(&model, &rev).into_iter().enumerate() {
        chunked_rev[texts.len() - 1 - i] = e;
    }
    let (min_cc, avg_cc) = drift_stats(&chunked, &chunked_rev);

    let bucketed_rev = embed_bucketed(&model, &rev);
    let (min_bb, avg_bb) = drift_stats(&bucketed, &{
        let mut v = vec![Vec::new(); texts.len()];
        for (i, e) in bucketed_rev.into_iter().enumerate() {
            v[texts.len() - 1 - i] = e;
        }
        v
    });

    let (min_bv, avg_bv) = drift_stats(&chunked, &bucketed);
    let (min_sc, avg_sc) = drift_stats(&solo, &chunked);
    let (min_sb, avg_sb) = drift_stats(&solo, &bucketed);

    eprintln!("=== code model, real code corpus ===");
    eprintln!(
        "solo wall: {}ms   chunked wall: {}ms   bucketed wall: {}ms",
        solo_t.as_millis(),
        chunked_t.as_millis(),
        bucketed_t.as_millis()
    );
    eprintln!(
        "chunked vs solo (canonicity): min={:.4} avg={:.4}",
        min_sc, avg_sc
    );
    eprintln!(
        "bucketed vs solo (canonicity): min={:.4} avg={:.4}",
        min_sb, avg_sb
    );
    eprintln!(
        "chunked fwd-vs-rev (composition drift): min={:.4} avg={:.4}",
        min_cc, avg_cc
    );
    eprintln!(
        "bucketed fwd-vs-rev (determinism):      min={:.4} avg={:.4}",
        min_bb, avg_bb
    );
    eprintln!(
        "bucketed-vs-chunked (vector shift):     min={:.4} avg={:.4}",
        min_bv, avg_bv
    );
}

// The re-embed normalization utility moved to
// `examples/reembed_code_entities.rs` — it mutates a live DB and is a
// tool, not a test.
