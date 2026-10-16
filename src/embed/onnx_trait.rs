//!  trait impl for .

use super::*;

impl EmbeddingModel for OnnxEmbeddingModel {
    fn embed(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
        if texts.is_empty() {
            return Ok(Vec::new());
        }

        self.try_load()?;

        // CodeRankEmbed's int8 kernels are not batch-invariant: the
        // same text drifts 0.03–0.09 cosine between batch sizes and
        // compositions (measured on real corpora), while queries
        // always embed solo. Per-text inference keeps stored vectors
        // canonical — deterministic across indexing patterns and
        // consistent with the query side — and is no slower on
        // measured corpora since code entities truncate to 512 and
        // every batch runs max-size attention anyway.
        if self.model_type == ModelType::Code {
            let mut all_results = Vec::with_capacity(texts.len());
            for &text in texts {
                all_results.extend(self.embed_chunk(&[text])?);
            }
            return Ok(all_results);
        }

        // Process in chunks to avoid excessive memory use on large
        // batches. 32 balances throughput and memory for 512-token
        // sequences with 768-dim embeddings. Larger batches (64+)
        // cause ORT's memory arena to grow beyond 5GB RSS.
        const CHUNK_SIZE: usize = 32;
        let mut all_results = Vec::with_capacity(texts.len());

        for chunk in texts.chunks(CHUNK_SIZE) {
            let embeddings = self.embed_chunk(chunk)?;
            all_results.extend(embeddings);
        }

        Ok(all_results)
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    fn model_name(&self) -> &str {
        self.model_type.model_name()
    }

    fn is_available(&self) -> bool {
        *self.available.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn embed_query(&self, texts: &[&str]) -> EmbeddingResult<Vec<Vec<f32>>> {
        let prefix = self.query_prefix();
        if prefix.is_empty() {
            return self.embed(texts);
        }
        let prefixed: Vec<String> = texts.iter().map(|t| format!("{}{}", prefix, t)).collect();
        let refs: Vec<&str> = prefixed.iter().map(|s| s.as_str()).collect();
        self.embed(&refs)
    }
}
