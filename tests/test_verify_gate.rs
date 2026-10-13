//! Calibration probe: the measurement that removed NLI claim
//! verification from `cogz verify` (item 96).
//!
//! Kept as reproducible evidence. Result on the live corpus (41
//! claim↔ref pairs): contradiction ≥0.70 on 19 pairs spread across the
//! whole code-embedding cosine range, entailment max 0.23 — code→prose
//! NLI is out-of-distribution and carries no usable verdict signal, so
//! the gate was removed rather than shipped. NLI remains for
//! consolidation, where the workload is NL→NL and was separately
//! benchmarked.
//!
//! Ignored by default — needs real ONNX models + the dogfood DB.
//! Run: `cargo test --test test_verify_gate -- --ignored --nocapture`

use cogz::embed::OnnxNliModel;
use cogz::embed::model::{EmbeddingModel, NliModel};
use cogz::embed::model_type::ModelType;
use cogz::embed::onnx::OnnxEmbeddingModel;

fn code_model() -> Option<OnnxEmbeddingModel> {
    let m = OnnxEmbeddingModel::new(ModelType::Code, &cogz::embed::models_dir(), 768);
    m.model_files_exist().then_some(m)
}

fn nli_model() -> Option<OnnxNliModel> {
    let m = OnnxNliModel::new(
        &cogz::embed::models_dir(),
        cogz::embed::registry::DEFAULT_NLI_MODEL,
    );
    m.model_files_exist().then_some(m)
}

#[test]
#[ignore = "needs real ONNX models + .cogz corpus; read-only"]
fn relatedness_calibration() {
    let Some(code) = code_model() else {
        eprintln!("code model not found — skipping");
        return;
    };
    let Some(nli) = nli_model() else {
        eprintln!("nli model not found — skipping");
        return;
    };
    let storage = cogz::storage::Storage::open(std::path::Path::new(".cogz/cogz.db"), 768).unwrap();

    // Living entities with declared refs — cap 40 for a bounded run.
    let claims: Vec<(String, String)> = {
        let conn = storage.conn();
        let mut s = conn
            .prepare(
                "SELECT e.id, substr(coalesce(e.title,'') || char(10) || coalesce(e.content,''), 1, 4000) \
                 FROM entities e \
                 WHERE e.status = 'active' \
                 AND e.type IN ('knowledge','observation') \
                 AND EXISTS (SELECT 1 FROM edges g WHERE g.source_id = e.id \
                             AND g.edge_type = 'references') \
                 ORDER BY e.id LIMIT 40",
            )
            .unwrap();
        s.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap()
    };

    let mut cosines: Vec<f64> = Vec::new();
    let mut pairs = 0usize;
    let mut scored = 0usize;
    let t0 = std::time::Instant::now();
    for (eid, claim) in &claims {
        let claim_vec = match code.embed(&[claim.as_str()]) {
            Ok(v) => v.into_iter().next().unwrap_or_default(),
            Err(_) => continue,
        };
        let (refs, embs): (Vec<(String, String)>, _) = {
            let conn = storage.conn();
            let mut s = conn
                .prepare(
                    "SELECT g.target_id, e.content FROM edges g \
                     JOIN entities e ON e.id = g.target_id \
                     WHERE g.source_id = ? AND g.edge_type = 'references' \
                     AND e.status = 'active' \
                     AND e.type IN ('function','class','file','module')",
                )
                .unwrap();
            let refs: Vec<(String, String)> = s
                .query_map([eid], |r| {
                    Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            let ids: Vec<String> = refs.iter().map(|(i, _)| i.clone()).collect();
            let embs = cogz::storage::embeddings::get_code_embeddings_batch(&conn, &ids)
                .unwrap_or_default();
            (refs, embs)
        };
        for (rid, content) in refs {
            let Some(ev) = embs.get(&rid) else { continue };
            let cos = cogz::embed::similarity::cosine_similarity(&claim_vec, ev);
            cosines.push(cos);
            pairs += 1;
            // Docstring premise: NL→NL NLI is in-distribution — test
            // whether comment-only premises rescue the signal.
            let doc: String = content
                .lines()
                .filter(|l| {
                    let t = l.trim_start();
                    t.starts_with("///") || t.starts_with("//!") || t.starts_with('#')
                })
                .map(|l| {
                    l.trim_start()
                        .trim_start_matches("//!")
                        .trim_start_matches("///")
                        .trim_start_matches('#')
                })
                .collect::<Vec<_>>()
                .join(" ");
            if cos >= 0.30 && scored < 60 && !doc.is_empty() {
                let p = match nli.classify(&doc.chars().take(2000).collect::<String>(), claim) {
                    Ok(p) => p,
                    Err(_) => continue,
                };
                scored += 1;
                eprintln!(
                    "cos={cos:.3}  DOC contra={:.3} entail={:.3}  claim={} ref={}",
                    p.contradiction,
                    p.entailment,
                    &eid[..8.min(eid.len())],
                    &rid[..8.min(rid.len())]
                );
            }
        }
    }
    cosines.sort_by(|a, b| a.partial_cmp(b).unwrap());
    eprintln!(
        "\n=== claim↔ref code-embedding cosine ({pairs} pairs, {scored} NLI-scored, {:.1}s) ===",
        t0.elapsed().as_secs_f64()
    );
    if cosines.is_empty() {
        eprintln!("no pairs — nothing to calibrate");
        return;
    }
    for q in [0.0, 0.1, 0.25, 0.5, 0.75, 0.9, 1.0] {
        let i = ((cosines.len() - 1) as f64 * q) as usize;
        eprintln!("p{:<4} {:.4}", (q * 100.0) as u32, cosines[i]);
    }
}
