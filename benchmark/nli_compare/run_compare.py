#!/usr/bin/env python3
"""NLI model comparison: current (deberta-v3-xsmall) vs Laurer large-anli.

Measures accuracy, per-class P/R/F1, latency, and peak RSS on a labeled
pair set. Same input pipeline as CogZ: input_ids + attention_mask only,
label order from config.json id2label.
"""
import json
import re
import resource
import sys
import time
from pathlib import Path

import numpy as np
import onnxruntime as ort
from tokenizers import Tokenizer

PAIRS = Path(__file__).parent / "pairs.jsonl"
LABELS = ["contradiction", "entailment", "neutral"]

MODELS = {
    "xsmall (current)": {
        "dir": Path.home() / ".local/share/cogz/models/models--cross-encoder--nli-deberta-v3-xsmall/snapshots/a150876415327c80daeff35ca6f68f5ed8cf5c24",
        "onnx": "onnx/model_quint8_avx2.onnx",
    },
    "laurer-large-anli": {
        "dir": Path(__file__).parent / "laurer-large",
        "onnx": "model_quantized.onnx",
    },
    "minilm2-l6": {
        "dir": Path(__file__).parent / "minilm2",
        "onnx": "onnx/model_quint8_avx2.onnx",
    },
    "tasksource-small": {
        "dir": Path(__file__).parent / "tasksource-small",
        "onnx": "model_quantized_v4.onnx",
    },
    "ts-small-prepackaged": {
        "dir": Path(__file__).parent / "ts-small-prepackaged",
        "onnx": "onnx/model_quantized.onnx",
    },
    "xsmall-tasksource": {
        "dir": Path(__file__).parent / "xsmall-tasksource",
        "onnx": "model_quantized.onnx",
    },
    "laurer-base-mfa": {
        "dir": Path(__file__).parent / "laurer-base-mfa",
        "onnx": "model_quantized.onnx",
    },
}


NUM_RE = re.compile(r"(?<![A-Za-z0-9])(\d+(?:\.\d+)?)\s*"
                    r"(seconds?|secs?|minutes?|mins?|hours?|days?|months?|years?|"
                    r"ms|milliseconds?|kb|mb|gb|bytes?|%|percent|hops?|tokens?|"
                    r"chars?|queries|files?|tasks?|terms?|docs?|seeds?)?", re.I)
TIME = {"second": 1, "sec": 1, "ms": 0.001, "millisecond": 0.001,
        "minute": 60, "min": 60, "hour": 3600, "day": 86400,
        "month": 2592000, "year": 31536000}
SIZE = {"byte": 1, "kb": 1024, "mb": 1048576, "gb": 1073741824}
STOP = set("the a an is are to of in on by for with and or that this it its as "
           "be can will must should not no only each every all any at from "
           "after before when if then than so such".split())
TOK_RE = re.compile(r"[a-z]+")


def canon(val, unit):
    u = (unit or "").lower().rstrip("s") or None
    if u in ("percent", "%"):
        return val / 100 if u == "percent" else val, "%"
    for tbl, cls in ((TIME, "time"), (SIZE, "size")):
        if u in tbl:
            return val * tbl[u], cls
    return val, u


def nums_and_slots(text):
    """[(canonical_value, unit_class, slot_token_set)] per numeric literal."""
    words = list(TOK_RE.finditer(text.lower()))
    spans = [w.span() for w in words]
    out = []
    for m in NUM_RE.finditer(text):
        toks = set()
        for i, (s, e) in enumerate(spans):
            if e <= m.start() and m.start() - e < 45:
                toks.add(words[i].group())
            elif s >= m.end() and s - m.end() < 45:
                toks.add(words[i].group())
        out.append((canon(float(m.group(1)), m.group(2)),
                    {t for t in toks if t not in STOP and len(t) > 2}))
    return out


def numeric_gate(premise, hypothesis):
    """Decide a pair when numbers fill the same definitional slot.

    contradiction: a number's slot context closely matches a slot on the
      other side but the canonical value differs ("X days" vs "Y days").
    entailment: slot matches and canonical values are equal ("90 days" vs
      "7776000 seconds" canonicalize equal).
    defers otherwise — consequences/paraphrases stay with the NLI model.
    """
    np_, nh = nums_and_slots(premise), nums_and_slots(hypothesis)
    if not np_ or not nh:
        return None
    best_c, best_e = 0.0, 0.0
    typed = []  # same-unit-class comparisons, (v1, v2)
    for v1, s1 in np_:
        for v2, s2 in nh:
            if v1[1] == v2[1] and v1[1] in ("time", "size", "%"):
                typed.append((v1[0], v2[0]))
            if not s1 or not s2 or v1[1] != v2[1]:
                continue
            jac = len(s1 & s2) / len(s1 | s2)
            if jac >= 0.65:
                if v1[0] != v2[0]:
                    best_c = max(best_c, jac)
                else:
                    best_e = max(best_e, jac)
    if best_c >= 0.65:
        return "contradiction"
    if best_e >= 0.6:
        return "entailment"
    if typed:
        tp = {t for t in TOK_RE.findall(premise.lower()) if t not in STOP and len(t) > 2}
        th = {t for t in TOK_RE.findall(hypothesis.lower()) if t not in STOP and len(t) > 2}
        tjac = len(tp & th) / max(1, len(tp | th))
        if tjac >= 0.4:
            if any(a != b for a, b in typed):
                return "contradiction"
            return "entailment"
    return None


def label_map(config_path):
    cfg = json.loads(Path(config_path).read_text())
    m = {}
    for idx, name in cfg["id2label"].items():
        n = name.lower()
        if "contrad" in n:
            m["contradiction"] = int(idx)
        elif "entail" in n:
            m["entailment"] = int(idx)
        elif "neutral" in n:
            m["neutral"] = int(idx)
    return m


def rss_mb():
    return resource.getrusage(resource.RUSAGE_SELF).ru_maxrss / 1024


def run(name, cfg, pairs):
    tok = Tokenizer.from_file(str(cfg["dir"] / "tokenizer.json"))
    tok.no_padding()
    tok.enable_truncation(max_length=512)
    lm = label_map(cfg["dir"] / "config.json")

    before = rss_mb()
    sess = ort.InferenceSession(
        str(cfg["dir"] / cfg["onnx"]),
        providers=["CPUExecutionProvider"],
        sess_options=ort.SessionOptions(),
    )
    load_rss = rss_mb()
    inputs = {i.name for i in sess.get_inputs()}

    enc = [tok.encode(p["premise"], p["hypothesis"]) for p in pairs]
    def feed(e):
        return {"input_ids": np.array([e.ids], dtype=np.int64),
                "attention_mask": np.array([e.attention_mask], dtype=np.int64),
                "token_type_ids": np.array([e.type_ids], dtype=np.int64)}

    # warmup
    for e in enc[:3]:
        sess.run(None, {k: v for k, v in feed(e).items() if k in inputs})

    lat, preds, probs = [], [], []
    for e, p in zip(enc, pairs):
        feed_d = {k: v for k, v in feed(e).items() if k in inputs}
        t0 = time.perf_counter()
        out = sess.run(None, feed_d)[0][0]
        lat.append((time.perf_counter() - t0) * 1000)
        ex = np.exp(out - out.max())
        pr = ex / ex.sum()
        probs.append(pr)
        preds.append(max(lm, key=lambda k: pr[lm[k]]))

    # reverse direction (hypothesis -> premise) for symmetric scoring
    enc_r = [tok.encode(p["hypothesis"], p["premise"]) for p in pairs]
    probs_r = []
    for e in enc_r:
        out = sess.run(None, {k: v for k, v in feed(e).items() if k in inputs})[0][0]
        ex = np.exp(out - out.max())
        probs_r.append(ex / ex.sum())

    # dump raw probabilities for offline calibration analysis
    out_dir = Path(__file__).parent / "probs"
    out_dir.mkdir(exist_ok=True)
    safe = re.sub(r"[^a-z0-9]+", "_", name.lower()).strip("_")
    with (out_dir / f"{safe}.jsonl").open("w") as fh:
        for p, pr, prr in zip(pairs, probs, probs_r):
            fh.write(json.dumps({
                "gold": p["label"],
                "fwd": {k: float(pr[lm[k]]) for k in lm},
                "rev": {k: float(prr[lm[k]]) for k in lm},
            }) + "\n")

    # hybrid arm: symbolic numeric gate overrides NLI when it fires
    gated = []
    gate_fired, gate_correct = 0, 0
    for p, pred in zip(pairs, preds):
        g = numeric_gate(p["premise"], p["hypothesis"])
        if g:
            gate_fired += 1
            if g == p["label"]:
                gate_correct += 1
        gated.append(g or pred)
    hyb_correct = sum(1 for p, pr in zip(pairs, gated) if pr == p["label"])

    rows = {"C": [0] * 3, "E": [0] * 3, "N": [0] * 3}
    order = ["contradiction", "entailment", "neutral"]
    correct = 0
    for p, pred, pr in zip(pairs, preds, probs):
        gold = p["label"]
        li = "CEN".index(gold[0].upper())
        rows["CEN"[li]][order.index(pred)] += 1
        if pred == gold:
            correct += 1

    lat = np.array(lat)
    print(f"\n=== {name} ===")
    print(f"rss_after_load={load_rss - before:.0f}MB delta, peak_total={rss_mb():.0f}MB")
    print(f"latency ms/pair: p50={np.percentile(lat,50):.0f} p95={np.percentile(lat,95):.0f}")
    print(f"accuracy: {correct}/{len(pairs)} = {correct/len(pairs):.3f}"
          f" | +gate: {hyb_correct}/{len(pairs)} = {hyb_correct/len(pairs):.3f}"
          f" (gate fired {gate_fired}x, {gate_correct} correct)")
    print(f"{'':14}{'predC':>6}{'predE':>6}{'predN':>6}   P      R")
    for i, g in enumerate("CEN"):
        tp = rows[g][i]
        pr_p = tp / sum(rows[g]) if sum(rows[g]) else 0
        rec = tp / sum(r[i] for r in rows.values()) if sum(r[i] for r in rows.values()) else 0
        print(f"gold={g:12}{rows[g][0]:>6}{rows[g][1]:>6}{rows[g][2]:>6}  {pr_p:.2f}  {rec:.2f}")

    # show misclassifications
    for p, pred, pr in zip(pairs, preds, probs):
        if pred != p["label"]:
            conf = pr[lm[pred]]
            print(f"  MISS gold={p['label'][:1]} pred={pred[:1]} conf={conf:.2f} :: {p['premise'][:55]} || {p['hypothesis'][:55]}")


def main():
    pairs = [json.loads(l) for l in PAIRS.read_text().splitlines() if l.strip()]
    only = sys.argv[1] if len(sys.argv) > 1 else None
    for name, cfg in MODELS.items():
        if only and only not in name:
            continue
        run(name, cfg, pairs)


if __name__ == "__main__":
    main()
