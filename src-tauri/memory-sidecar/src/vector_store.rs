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

        // Vector search with user filter
        let query = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?
            .limit(limit)
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

        debug!("Found {} memories", entries.len());
        Ok(entries)
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

        // Vector search without user filter
        let query = table
            .vector_search(query_embedding.to_vec())
            .context("Failed to create vector search")?
            .limit(limit);

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

        debug!("Found {} memories", entries.len());
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
