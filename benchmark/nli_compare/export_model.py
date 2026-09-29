#!/usr/bin/env python3
"""Export HF seq-classification model to ONNX + dynamic int8 quantization."""
import sys
from pathlib import Path

import torch
from transformers import AutoModelForSequenceClassification, AutoTokenizer
from onnxruntime.quantization import quantize_dynamic, QuantType


def export(repo, out_dir):
    out = Path(out_dir)
    out.mkdir(parents=True, exist_ok=True)

    tok = AutoTokenizer.from_pretrained(repo)
    tok.save_pretrained(out)

    model = AutoModelForSequenceClassification.from_pretrained(repo)
    model = model.float()
    model.eval()

    ids = torch.randint(0, 1000, (1, 16))
    mask = torch.ones_like(ids)
    tids = torch.zeros_like(ids)
    fp = out / "model.onnx"
    torch.onnx.export(
        model, (ids, mask, tids), str(fp),
        input_names=["input_ids", "attention_mask", "token_type_ids"],
        output_names=["logits"],
        dynamic_axes={
            "input_ids": {0: "b", 1: "s"},
            "attention_mask": {0: "b", 1: "s"},
            "token_type_ids": {0: "b", 1: "s"},
            "logits": {0: "b"},
        },
        opset_version=17,
        dynamo=False,
    )
    print(f"exported {fp} ({fp.stat().st_size / 1e6:.0f}MB)")

    qp = out / "model_quantized.onnx"
    import onnx
    m = onnx.load(str(fp), load_external_data=False)
    attn_gathers = [n.name for n in m.graph.node
                    if n.op_type == "Gather" and "encoder" in n.name]
    quantize_dynamic(str(fp), str(qp), weight_type=QuantType.QInt8,
                     nodes_to_exclude=attn_gathers)
    print(f"quantized {qp} ({qp.stat().st_size / 1e6:.0f}MB), "
          f"excluded {len(attn_gathers)} gathers")
    fp.unlink()


if __name__ == "__main__":
    export(sys.argv[1], sys.argv[2])
