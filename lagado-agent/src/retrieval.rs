//! retrieval.rs — RAG context retrieval for hydra.
//!
//! Phase 1: TF-IDF-like word-overlap scoring (no embedding model, no FAISS).
//! Phase 2: swap score_similarity() to use embedding-vector cosine distance.
//!
//! Two responsibilities:
//!   1. retrieve_context()  — K most relevant memory entries for the current query
//!   2. select_tools()      — K=15 most relevant tools from the registry

use std::collections::HashSet;

#[derive(Debug, Clone)]
pub struct RetrievedEntry {
    pub text:        String,
    pub score:       f32,    // 0.0–1.0 relevance
    pub source:      Source,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    MemoryHot,
    MemoryWarm,
    MemoryCold,
    ActionGraph,
}

pub struct Retriever {
    memory_db_path:       std::path::PathBuf,
    action_graph_db_path: std::path::PathBuf,
}

impl Retriever {
    pub fn new(data_dir: &std::path::Path) -> Self {
        Self {
            memory_db_path:       data_dir.join("memory.db"),
            action_graph_db_path: data_dir.join("action_graph.db"),
        }
    }

    /// Compute relevance score between a query and a candidate text.
    /// Phase 1: Jaccard-like word overlap with length penalty for very short matches.
    /// Phase 2: replace body with embedding cosine similarity.
    fn score_similarity(query: &str, candidate: &str) -> f32 {
        let q_words: HashSet<&str> =
            query.split_whitespace().collect();
        let c_words: HashSet<&str> =
            candidate.split_whitespace().collect();
        if q_words.is_empty() || c_words.is_empty() {
            return 0.0;
        }
        let intersection = q_words.intersection(&c_words).count() as f32;
        let union = q_words.union(&c_words).count() as f32;
        let jaccard = intersection / union;
        // Boost entries that are longer (more informative)
        let length_factor = (c_words.len() as f32 / 20.0_f32).min(1.0);
        (jaccard * 0.8 + length_factor * 0.2).min(1.0)
    }

    /// Retrieve the K most relevant entries across all memory tiers for the query.
    pub fn retrieve_context(&self, query: &str, k: usize) -> Vec<RetrievedEntry> {
        let mut entries: Vec<RetrievedEntry> = Vec::new();

        // Pull from memory_tiers SQLite (warm + cold)
        if let Ok(conn) = rusqlite::Connection::open(&self.memory_db_path) {
            let sql = "SELECT text, tier FROM memory_entries ORDER BY temperature DESC LIMIT 200";
            if let Ok(mut stmt) = conn.prepare(sql) {
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                });
                if let Ok(rows) = rows {
                    for row in rows.flatten() {
                        let (text, tier_str) = row;
                        let score = Self::score_similarity(query, &text);
                        let source = match tier_str.as_str() {
                            "warm" => Source::MemoryWarm,
                            "cold" => Source::MemoryCold,
                            _      => Source::MemoryHot,
                        };
                        entries.push(RetrievedEntry { text, score, source });
                    }
                }
            }
        }

        // Pull recent action_graph outcomes as context
        if let Ok(conn) = rusqlite::Connection::open(&self.action_graph_db_path) {
            let sql = "SELECT state_hash, action_json FROM action_edges
                       ORDER BY probability DESC LIMIT 50";
            if let Ok(mut stmt) = conn.prepare(sql) {
                let rows = stmt.query_map([], |row| {
                    Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
                });
                if let Ok(rows) = rows {
                    for row in rows.flatten() {
                        let (_, action) = row;
                        let score = Self::score_similarity(query, &action);
                        entries.push(RetrievedEntry {
                            text: action,
                            score,
                            source: Source::ActionGraph,
                        });
                    }
                }
            }
        }

        // Sort by score descending, take K
        entries.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        entries.truncate(k);
        entries
    }

    /// Select up to k tools from the registry most relevant to the query.
    /// Always includes all tools if k >= registry size (Phase 1: only 5 core tools).
    pub fn select_tools(
        &self,
        query: &str,
        tools: &[crate::operator::ToolDescriptor],
        k: usize,
    ) -> Vec<crate::operator::ToolDescriptor> {
        if tools.len() <= k {
            return tools.to_vec();
        }
        let mut scored: Vec<(f32, &crate::operator::ToolDescriptor)> = tools
            .iter()
            .map(|t| {
                let combined = format!("{} {}", t.name, t.description);
                (Self::score_similarity(query, &combined), t)
            })
            .collect();
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.into_iter().take(k).map(|(_, t)| t.clone()).collect()
    }

    /// Format retrieved entries as a context string for prompt injection.
    pub fn format_context(entries: &[RetrievedEntry]) -> String {
        if entries.is_empty() {
            return String::new();
        }
        let lines: Vec<String> = entries
            .iter()
            .filter(|e| e.score > 0.0)
            .map(|e| format!("- {}", e.text.trim()))
            .collect();
        if lines.is_empty() {
            return String::new();
        }
        format!("Relevant context:\n{}", lines.join("\n"))
    }
}
