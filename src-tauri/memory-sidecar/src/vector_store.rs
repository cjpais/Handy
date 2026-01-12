//! Vector store using LanceDB for semantic memory storage
//!
//! Stores conversation messages with embeddings for semantic search.
//! Supports per-user memory with TTL-based expiration.

use anyhow::{Context, Result};
use arrow_array::{
    ArrayRef, Float32Array, Int64Array, RecordBatch, RecordBatchIterator,
    StringArray, BooleanArray, builder::FixedSizeListBuilder, builder::Float32Builder,
};
use arrow_schema::{DataType, Field, Schema};
use chrono::Utc;
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase};
use lancedb::{connect, Connection, Table};
use log::{debug, info};
use serde::{Deserialize, Serialize};
use std::path::Path;
use std::sync::Arc;

const TABLE_NAME: &str = "memories";
const EMBEDDING_DIM: i32 = 384;

/// Minimum similarity threshold for query results (0.0 to 1.0)
/// Results with similarity below this are filtered out
/// 0.3 is a reasonable threshold - too low includes irrelevant results,
/// too high excludes potentially useful context
const MIN_SIMILARITY_THRESHOLD: f32 = 0.3;

/// Recency decay factor - how much to weight recent memories over old ones
/// Higher values = more recency bias. 0.0 = no recency weighting
const RECENCY_WEIGHT: f32 = 0.3;

/// Time constant for recency decay in seconds (7 days)
/// Memories older than this get diminishing recency bonus
const RECENCY_HALF_LIFE_SECS: f64 = 7.0 * 24.0 * 60.0 * 60.0;

/// Minimum content length difference to consider memories as duplicates
/// (used with Levenshtein-like comparison)
const DEDUP_SIMILARITY_THRESHOLD: f32 = 0.85;

/// A memory entry stored in the database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryEntry {
    pub id: String,
    pub user_id: String,
    pub content: String,
    pub is_bot: bool,
    pub timestamp: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub similarity: Option<f32>,
}

/// Calculate recency score (0.0 to 1.0) based on timestamp age
/// More recent = higher score
fn calculate_recency_score(timestamp: i64) -> f32 {
    let now = Utc::now().timestamp();
    let age_secs = (now - timestamp).max(0) as f64;

    // Exponential decay: score = e^(-age / half_life)
    // This gives 1.0 for now, 0.5 at half_life, 0.25 at 2*half_life, etc.
    let decay = (-age_secs / RECENCY_HALF_LIFE_SECS).exp();
    decay as f32
}

/// Calculate combined score from similarity and recency
/// final_score = similarity * (1 - recency_weight) + recency * recency_weight
fn calculate_combined_score(similarity: f32, recency: f32) -> f32 {
    similarity * (1.0 - RECENCY_WEIGHT) + recency * RECENCY_WEIGHT
}

/// Calculate text similarity for deduplication (simple normalized comparison)
/// Returns 0.0 to 1.0 where 1.0 = identical
fn text_similarity(a: &str, b: &str) -> f32 {
    let a_lower = a.to_lowercase();
    let b_lower = b.to_lowercase();

    if a_lower == b_lower {
        return 1.0;
    }

    // Simple character overlap similarity (Jaccard-like)
    let a_chars: std::collections::HashSet<char> = a_lower.chars().collect();
    let b_chars: std::collections::HashSet<char> = b_lower.chars().collect();

    let intersection = a_chars.intersection(&b_chars).count();
    let union = a_chars.union(&b_chars).count();

    if union == 0 {
        0.0
    } else {
        intersection as f32 / union as f32
    }
}

/// Deduplicate memory entries, keeping the highest-scored version of similar content
fn deduplicate_entries(mut entries: Vec<MemoryEntry>) -> Vec<MemoryEntry> {
    if entries.len() <= 1 {
        return entries;
    }

    let mut result: Vec<MemoryEntry> = Vec::new();

    for entry in entries.drain(..) {
        let is_duplicate = result.iter().any(|existing| {
            text_similarity(&existing.content, &entry.content) >= DEDUP_SIMILARITY_THRESHOLD
        });

        if !is_duplicate {
            result.push(entry);
        }
    }

    result
}

/// Vector store for conversation memories
pub struct VectorStore {
    db: Connection,
    table: Option<Table>,
}

impl VectorStore {
    /// Open or create the vector store at the given path
    pub async fn open(db_path: &Path) -> Result<Self> {
        info!("Opening vector store at: {:?}", db_path);

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent).context("Failed to create database directory")?;
        }

        let db = connect(db_path.to_string_lossy().as_ref())
            .execute()
            .await
            .context("Failed to connect to LanceDB")?;

        let mut store = Self { db, table: None };

        // Try to open existing table or create new one
        store.ensure_table().await?;

        Ok(store)
    }

    /// Ensure the memories table exists
    async fn ensure_table(&mut self) -> Result<()> {
        let table_names = self.db.table_names().execute().await?;

        if table_names.contains(&TABLE_NAME.to_string()) {
            debug!("Opening existing table: {}", TABLE_NAME);
            self.table = Some(
                self.db
                    .open_table(TABLE_NAME)
                    .execute()
                    .await
                    .context("Failed to open existing table")?,
            );
        } else {
            info!("Creating new table: {}", TABLE_NAME);
            // Create with empty initial data - will be populated on first insert
            self.table = None;
        }

        Ok(())
    }

    /// Create the table schema
    fn create_schema() -> Schema {
        Schema::new(vec![
            Field::new("id", DataType::Utf8, false),
            Field::new("user_id", DataType::Utf8, false),
            Field::new("content", DataType::Utf8, false),
            Field::new("is_bot", DataType::Boolean, false),
            Field::new("timestamp", DataType::Int64, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    EMBEDDING_DIM,
                ),
                false,
            ),
        ])
    }

    /// Store a message with its embedding
    pub async fn store(
        &mut self,
        id: &str,
        user_id: &str,
        content: &str,
        embedding: &[f32],
        is_bot: bool,
    ) -> Result<()> {
        let timestamp = Utc::now().timestamp();

        debug!(
            "Storing memory: id={}, user_id={}, is_bot={}",
            id, user_id, is_bot
        );

        // Create record batch
        let schema = Arc::new(Self::create_schema());

        let id_array = Arc::new(StringArray::from(vec![id])) as ArrayRef;
        let user_id_array = Arc::new(StringArray::from(vec![user_id])) as ArrayRef;
        let content_array = Arc::new(StringArray::from(vec![content])) as ArrayRef;
        let is_bot_array = Arc::new(BooleanArray::from(vec![is_bot])) as ArrayRef;
        let timestamp_array = Arc::new(Int64Array::from(vec![timestamp])) as ArrayRef;

        // Create fixed-size list for embedding using builder
        let mut list_builder = FixedSizeListBuilder::new(Float32Builder::new(), EMBEDDING_DIM);
        let values_builder = list_builder.values();
        for &val in embedding {
            values_builder.append_value(val);
        }
        list_builder.append(true);
        let vector_array = Arc::new(list_builder.finish()) as ArrayRef;

        let batch = RecordBatch::try_new(
            schema.clone(),
            vec![
                id_array,
                user_id_array,
                content_array,
                is_bot_array,
                timestamp_array,
                vector_array,
            ],
        )
        .context("Failed to create record batch")?;

        // Add to table (create if doesn't exist)
        if self.table.is_none() {
            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            self.table = Some(
                self.db
                    .create_table(TABLE_NAME, batches)
                    .execute()
                    .await
                    .context("Failed to create table")?,
            );
        } else {
            let table = self.table.as_ref().unwrap();
            let batches = RecordBatchIterator::new(vec![Ok(batch)], schema);
            table
                .add(batches)
                .execute()
                .await
                .context("Failed to add to table")?;
        }

        Ok(())
    }

    /// Query for similar messages for a user
    pub async fn query(
        &self,
        user_id: &str,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                debug!("No table exists yet, returning empty results");
                return Ok(vec![]);
            }
        };

        debug!("Querying memories for user: {}", user_id);

        // Fetch more candidates than requested, then post-filter for quality
        // This compensates for filtering/deduplication removing some results
        let search_limit = (limit * 3).max(10);

        // Vector search with user filter
        let query = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?
            .limit(search_limit)
            .only_if(format!("user_id = '{}'", user_id));

        let results = query
            .execute()
            .await
            .context("Failed to execute vector search")?
            .try_collect::<Vec<_>>()
            .await
            .context("Failed to collect results")?;

        let mut entries = Vec::new();

        for batch in results {
            let id_col = batch
                .column_by_name("id")
                .context("Missing id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid id column type")?;

            let user_id_col = batch
                .column_by_name("user_id")
                .context("Missing user_id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid user_id column type")?;

            let content_col = batch
                .column_by_name("content")
                .context("Missing content column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid content column type")?;

            let is_bot_col = batch
                .column_by_name("is_bot")
                .context("Missing is_bot column")?
                .as_any()
                .downcast_ref::<BooleanArray>()
                .context("Invalid is_bot column type")?;

            let timestamp_col = batch
                .column_by_name("timestamp")
                .context("Missing timestamp column")?
                .as_any()
                .downcast_ref::<Int64Array>()
                .context("Invalid timestamp column type")?;

            // Distance is added by vector search
            let distance_col = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

            for i in 0..batch.num_rows() {
                let similarity = distance_col.map(|d| {
                    // Convert distance to similarity (lower distance = higher similarity)
                    // LanceDB uses L2 distance by default
                    let dist = d.value(i);
                    1.0 / (1.0 + dist)
                });

                entries.push(MemoryEntry {
                    id: id_col.value(i).to_string(),
                    user_id: user_id_col.value(i).to_string(),
                    content: content_col.value(i).to_string(),
                    is_bot: is_bot_col.value(i),
                    timestamp: timestamp_col.value(i),
                    similarity,
                });
            }
        }

        // Filter by minimum similarity threshold
        let total_before_filter = entries.len();
        let mut filtered_entries: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| e.similarity.unwrap_or(0.0) >= MIN_SIMILARITY_THRESHOLD)
            .collect();

        // Apply recency weighting and re-sort by combined score
        // This promotes recent relevant memories over old ones
        filtered_entries.sort_by(|a, b| {
            let a_sim = a.similarity.unwrap_or(0.0);
            let b_sim = b.similarity.unwrap_or(0.0);
            let a_recency = calculate_recency_score(a.timestamp);
            let b_recency = calculate_recency_score(b.timestamp);
            let a_combined = calculate_combined_score(a_sim, a_recency);
            let b_combined = calculate_combined_score(b_sim, b_recency);
            // Sort descending (higher score first)
            b_combined.partial_cmp(&a_combined).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate similar content (keeps first occurrence = highest scored)
        let mut deduped_entries = deduplicate_entries(filtered_entries);

        // Truncate to requested limit
        deduped_entries.truncate(limit);

        debug!(
            "Found {} memories ({} after filter/dedup, returning {})",
            total_before_filter,
            deduped_entries.len(),
            deduped_entries.len().min(limit)
        );
        Ok(deduped_entries)
    }

    /// Delete messages older than TTL days
    pub async fn cleanup(&mut self, ttl_days: u32) -> Result<u32> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                debug!("No table exists yet, nothing to cleanup");
                return Ok(0);
            }
        };

        let cutoff = Utc::now().timestamp() - (ttl_days as i64 * 24 * 60 * 60);
        info!("Cleaning up memories older than {} days (cutoff: {})", ttl_days, cutoff);

        // Count before delete
        let count_before = table
            .count_rows(Some(format!("timestamp < {}", cutoff)))
            .await
            .unwrap_or(0);

        if count_before > 0 {
            table
                .delete(&format!("timestamp < {}", cutoff))
                .await
                .context("Failed to delete old memories")?;
        }

        info!("Deleted {} old memories", count_before);
        Ok(count_before as u32)
    }

    /// Get total count of memories for a user
    #[allow(dead_code)]
    pub async fn count_for_user(&self, user_id: &str) -> Result<usize> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(0),
        };

        let count = table
            .count_rows(Some(format!("user_id = '{}'", user_id)))
            .await
            .context("Failed to count rows")?;

        Ok(count)
    }

    /// Get total count of all memories
    pub async fn count_all(&self) -> Result<usize> {
        let table = match &self.table {
            Some(t) => t,
            None => return Ok(0),
        };

        let count = table
            .count_rows(None::<String>)
            .await
            .context("Failed to count rows")?;

        Ok(count)
    }

    /// Query for similar messages across all users
    pub async fn query_all(
        &self,
        query_embedding: &[f32],
        limit: usize,
    ) -> Result<Vec<MemoryEntry>> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                debug!("No table exists yet, returning empty results");
                return Ok(vec![]);
            }
        };

        debug!("Querying all memories");

        // Fetch more candidates than requested for post-filtering
        let search_limit = (limit * 3).max(10);

        // Vector search without user filter
        let query = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?
            .limit(search_limit);

        let results = query
            .execute()
            .await
            .context("Failed to execute vector search")?
            .try_collect::<Vec<_>>()
            .await
            .context("Failed to collect results")?;

        let mut entries = Vec::new();

        for batch in results {
            let id_col = batch
                .column_by_name("id")
                .context("Missing id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid id column type")?;

            let user_id_col = batch
                .column_by_name("user_id")
                .context("Missing user_id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid user_id column type")?;

            let content_col = batch
                .column_by_name("content")
                .context("Missing content column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid content column type")?;

            let is_bot_col = batch
                .column_by_name("is_bot")
                .context("Missing is_bot column")?
                .as_any()
                .downcast_ref::<BooleanArray>()
                .context("Invalid is_bot column type")?;

            let timestamp_col = batch
                .column_by_name("timestamp")
                .context("Missing timestamp column")?
                .as_any()
                .downcast_ref::<Int64Array>()
                .context("Invalid timestamp column type")?;

            // Distance is added by vector search
            let distance_col = batch
                .column_by_name("_distance")
                .and_then(|c| c.as_any().downcast_ref::<Float32Array>());

            for i in 0..batch.num_rows() {
                let similarity = distance_col.map(|d| {
                    let dist = d.value(i);
                    1.0 / (1.0 + dist)
                });

                entries.push(MemoryEntry {
                    id: id_col.value(i).to_string(),
                    user_id: user_id_col.value(i).to_string(),
                    content: content_col.value(i).to_string(),
                    is_bot: is_bot_col.value(i),
                    timestamp: timestamp_col.value(i),
                    similarity,
                });
            }
        }

        // Filter by minimum similarity threshold
        let total_before_filter = entries.len();
        let mut filtered_entries: Vec<MemoryEntry> = entries
            .into_iter()
            .filter(|e| e.similarity.unwrap_or(0.0) >= MIN_SIMILARITY_THRESHOLD)
            .collect();

        // Apply recency weighting and re-sort by combined score
        filtered_entries.sort_by(|a, b| {
            let a_sim = a.similarity.unwrap_or(0.0);
            let b_sim = b.similarity.unwrap_or(0.0);
            let a_recency = calculate_recency_score(a.timestamp);
            let b_recency = calculate_recency_score(b.timestamp);
            let a_combined = calculate_combined_score(a_sim, a_recency);
            let b_combined = calculate_combined_score(b_sim, b_recency);
            b_combined.partial_cmp(&a_combined).unwrap_or(std::cmp::Ordering::Equal)
        });

        // Deduplicate similar content
        let mut deduped_entries = deduplicate_entries(filtered_entries);

        // Truncate to requested limit
        deduped_entries.truncate(limit);

        debug!(
            "Found {} memories ({} after filter/dedup, returning {})",
            total_before_filter,
            deduped_entries.len(),
            deduped_entries.len().min(limit)
        );
        Ok(deduped_entries)
    }

    /// Get list of unique user IDs with memory counts
    pub async fn list_users(&self) -> Result<Vec<(String, usize)>> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                debug!("No table exists yet, returning empty user list");
                return Ok(vec![]);
            }
        };

        debug!("Listing unique users");

        // Query all rows to extract unique user_ids
        // LanceDB doesn't have GROUP BY, so we fetch all and aggregate in memory
        let results = table
            .query()
            .select(lancedb::query::Select::Columns(vec!["user_id".to_string()]))
            .execute()
            .await
            .context("Failed to query user_ids")?
            .try_collect::<Vec<_>>()
            .await
            .context("Failed to collect user_ids")?;

        let mut user_counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();

        for batch in results {
            let user_id_col = batch
                .column_by_name("user_id")
                .context("Missing user_id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid user_id column type")?;

            for i in 0..batch.num_rows() {
                let user_id = user_id_col.value(i).to_string();
                *user_counts.entry(user_id).or_insert(0) += 1;
            }
        }

        let mut users: Vec<(String, usize)> = user_counts.into_iter().collect();
        // Sort by count descending
        users.sort_by(|a, b| b.1.cmp(&a.1));

        debug!("Found {} unique users", users.len());
        Ok(users)
    }

    /// Browse recent memories without semantic search (for browsing UI)
    pub async fn browse_recent(
        &self,
        limit: usize,
        user_filter: Option<&str>,
        is_bot_filter: Option<bool>,
    ) -> Result<Vec<MemoryEntry>> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                debug!("No table exists yet, returning empty results");
                return Ok(vec![]);
            }
        };

        debug!("Browsing recent memories: limit={}, user={:?}, is_bot={:?}", limit, user_filter, is_bot_filter);

        // Build filter condition
        let mut conditions: Vec<String> = Vec::new();
        if let Some(user_id) = user_filter {
            conditions.push(format!("user_id = '{}'", user_id));
        }
        if let Some(is_bot) = is_bot_filter {
            conditions.push(format!("is_bot = {}", is_bot));
        }

        let query = table.query();
        let query = if conditions.is_empty() {
            query
        } else {
            query.only_if(conditions.join(" AND "))
        };

        let results = query
            .limit(limit * 2) // Fetch extra for post-processing
            .execute()
            .await
            .context("Failed to execute browse query")?
            .try_collect::<Vec<_>>()
            .await
            .context("Failed to collect results")?;

        let mut entries = Vec::new();

        for batch in results {
            let id_col = batch
                .column_by_name("id")
                .context("Missing id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid id column type")?;

            let user_id_col = batch
                .column_by_name("user_id")
                .context("Missing user_id column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid user_id column type")?;

            let content_col = batch
                .column_by_name("content")
                .context("Missing content column")?
                .as_any()
                .downcast_ref::<StringArray>()
                .context("Invalid content column type")?;

            let is_bot_col = batch
                .column_by_name("is_bot")
                .context("Missing is_bot column")?
                .as_any()
                .downcast_ref::<BooleanArray>()
                .context("Invalid is_bot column type")?;

            let timestamp_col = batch
                .column_by_name("timestamp")
                .context("Missing timestamp column")?
                .as_any()
                .downcast_ref::<Int64Array>()
                .context("Invalid timestamp column type")?;

            for i in 0..batch.num_rows() {
                entries.push(MemoryEntry {
                    id: id_col.value(i).to_string(),
                    user_id: user_id_col.value(i).to_string(),
                    content: content_col.value(i).to_string(),
                    is_bot: is_bot_col.value(i),
                    timestamp: timestamp_col.value(i),
                    similarity: None,
                });
            }
        }

        // Sort by timestamp descending (most recent first)
        entries.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
        entries.truncate(limit);

        debug!("Returning {} recent memories", entries.len());
        Ok(entries)
    }

    /// Clear all memories
    pub async fn clear_all(&mut self) -> Result<u32> {
        let table = match &self.table {
            Some(t) => t,
            None => {
                debug!("No table exists yet, nothing to clear");
                return Ok(0);
            }
        };

        // Count before delete
        let count = table
            .count_rows(None::<String>)
            .await
            .unwrap_or(0);

        if count > 0 {
            // Delete all rows by using a condition that's always true
            table
                .delete("timestamp >= 0")
                .await
                .context("Failed to delete all memories")?;
        }

        info!("Cleared {} memories", count);

        // Reset the table reference since we cleared everything
        self.table = None;

        Ok(count as u32)
    }
}
