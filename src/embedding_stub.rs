//! Stub embedding module when the `embeddings` feature is disabled.
//!
//! Provides the same public API as the real embedding module but all
//! operations return errors or no-ops.

use anyhow::Result;
use std::time::Duration;

pub type EmbeddingVec = Vec<f32>;

#[derive(Debug, Clone)]
pub struct EmbedderStats {
    pub loaded: bool,
    pub load_count: u64,
    pub unload_count: u64,
    pub embed_calls: u64,
    pub embed_failures: u64,
    pub total_embed_ms: u64,
    pub avg_embed_ms: Option<f64>,
    pub idle_secs: Option<u64>,
    pub loaded_secs: Option<u64>,
    pub cache_hits: u64,
    pub cache_size: usize,
}

pub struct Embedder;

impl Embedder {
    pub fn load() -> Result<Self> {
        anyhow::bail!("Embeddings feature not compiled in this build")
    }
}

pub fn get_embedder() -> Result<std::sync::Arc<Embedder>> {
    anyhow::bail!("Embeddings feature not compiled in this build")
}

pub fn embed(_text: &str) -> Result<EmbeddingVec> {
    anyhow::bail!("Embeddings feature not compiled in this build")
}

pub fn maybe_unload_if_idle(_idle_for: Duration) -> bool {
    false
}

pub fn stats() -> EmbedderStats {
    EmbedderStats {
        loaded: false,
        load_count: 0,
        unload_count: 0,
        embed_calls: 0,
        embed_failures: 0,
        total_embed_ms: 0,
        avg_embed_ms: None,
        idle_secs: None,
        loaded_secs: None,
        cache_hits: 0,
        cache_size: 0,
    }
}

pub fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let dot: f32 = a.iter().zip(b.iter()).map(|(x, y)| x * y).sum();
    let norm_a: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let norm_b: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm_a == 0.0 || norm_b == 0.0 {
        return 0.0;
    }
    dot / (norm_a * norm_b)
}

pub fn batch_cosine_similarity(query: &[f32], candidates: &[&[f32]]) -> Vec<f32> {
    let dim = query.len();
    if dim == 0 || candidates.is_empty() {
        return vec![0.0; candidates.len()];
    }
    candidates
        .iter()
        .map(|c| {
            if c.len() != dim {
                0.0
            } else {
                c.iter().zip(query.iter()).map(|(a, b)| a * b).sum()
            }
        })
        .collect()
}

pub fn find_similar(
    query: &[f32],
    candidates: &[EmbeddingVec],
    threshold: f32,
    top_k: usize,
) -> Vec<(usize, f32)> {
    let refs: Vec<&[f32]> = candidates.iter().map(|v| v.as_slice()).collect();
    let scores = batch_cosine_similarity(query, &refs);
    let mut results: Vec<(usize, f32)> = scores
        .into_iter()
        .enumerate()
        .filter(|(_, score)| *score >= threshold)
        .collect();
    results.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    results.truncate(top_k);
    results
}

pub fn is_model_available() -> bool {
    false
}

pub const fn embedding_dim() -> usize {
    384
}
