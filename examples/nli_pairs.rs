//! Benchmark an NLI model on the labeled pair corpus through the real
//! OnnxNliModel pipeline — tokenizer, bidirectional scoring, the
//! numeric gate, and the merge-style entailment check. Usage:
//!
//!   cargo run --release --example nli_pairs -- \
//!     <models_dir> <model_id> [pairs.jsonl]

use std::path::Path;
use std::time::Instant;

use cogz::consolidate::dedup::confirm_duplicate_nli;
use cogz::consolidate::numgate::{self, GateDecision};
use cogz::embed::{NliModel, OnnxNliModel};

#[derive(serde::Deserialize)]
struct Pair {
    premise: String,
    hypothesis: String,
    #[serde(alias = "gold")]
    label: String,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let models_dir = args.next().expect("models_dir required");
    let model_id = args.next().expect("model_id required");
    let pairs_path = args
        .next()
        .unwrap_or_else(|| "benchmark/nli_compare/pairs.jsonl".to_string());

    let text = std::fs::read_to_string(&pairs_path)?;
    let pairs: Vec<Pair> = text
        .lines()
        .filter(|l| !l.trim().is_empty())
        .map(serde_json::from_str)
        .collect::<Result<_, _>>()?;

    let model = OnnxNliModel::new(Path::new(&models_dir), &model_id);
    println!("model={model_id} files_exist={}", model.model_files_exist());
    if !model.model_files_exist() {
        return Ok(());
    }

    let mut lat_ms = Vec::new();
    let (mut n, mut argmax_ok) = (0usize, 0usize);
    let (mut c_tp, mut c_fp, mut c_fn) = (0usize, 0usize, 0usize);
    let (mut e_tp, mut e_fp, mut e_fn) = (0usize, 0usize, 0usize);
    let (mut gate_fired, mut gate_ok) = (0usize, 0usize);

    for p in &pairs {
        let gate = numgate::decide(&p.premise, &p.hypothesis);

        let t0 = Instant::now();
        let fwd = model.classify(&p.premise, &p.hypothesis);
        let rev = model.classify(&p.hypothesis, &p.premise);
        lat_ms.push(t0.elapsed().as_secs_f64() * 1000.0 / 2.0);
        let (Ok(f), Ok(r)) = (fwd, rev) else {
            continue;
        };

        n += 1;
        let labels = ["contradiction", "entailment", "neutral"];
        let scores = [f.contradiction, f.entailment, f.neutral];
        let argmax = labels[scores
            .iter()
            .enumerate()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(i, _)| i)
            .unwrap()];
        if argmax == p.label {
            argmax_ok += 1;
        }

        // Contradiction flagging: gate overrides NLI; otherwise
        // max-direction P(C) at the production threshold.
        let max_c = f.contradiction.max(r.contradiction);
        let pred_c = gate == Some(GateDecision::Contradiction) || max_c >= 0.70;
        let gold_c = p.label == "contradiction";
        match (pred_c, gold_c) {
            (true, true) => c_tp += 1,
            (true, false) => c_fp += 1,
            (false, true) => c_fn += 1,
            _ => {}
        }

        if let Some(g) = gate {
            gate_fired += 1;
            let label = match g {
                GateDecision::Contradiction => "contradiction",
                GateDecision::Equivalent => "entailment",
            };
            if label == p.label {
                gate_ok += 1;
            }
        }

        // Merge-style confirmation: max-direction entailment at the
        // calibrated 0.55 threshold.
        let gold_e = p.label == "entailment";
        let pred_e = confirm_duplicate_nli(Some(&model), &p.premise, &p.hypothesis, 0.55);
        match (pred_e, gold_e) {
            (true, true) => e_tp += 1,
            (true, false) => e_fp += 1,
            (false, true) => e_fn += 1,
            _ => {}
        }
    }

    lat_ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let p50 = lat_ms.get(lat_ms.len() / 2).copied().unwrap_or(0.0);
    let pr = |tp: usize, fp: usize, fn_: usize| {
        let p = tp as f64 / (tp + fp).max(1) as f64;
        let r = tp as f64 / (tp + fn_).max(1) as f64;
        format!("P={p:.2} R={r:.2}")
    };

    println!(
        "pairs={n} argmax_acc={argmax_ok} ({:.3})",
        argmax_ok as f64 / n as f64
    );
    println!("contradiction@0.70+gate: {}", pr(c_tp, c_fp, c_fn));
    println!("merge-entailment@0.55:  {}", pr(e_tp, e_fp, e_fn));
    println!("gate: fired={gate_fired} correct={gate_ok}");
    println!("latency p50={:.0}ms (per-direction)", p50);
    Ok(())
}
