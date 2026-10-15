//! Embedding operations — vec0 vector storage for semantic search.
//!
//! Code and knowledge entities use different embedding models with
//! incompatible vector spaces. Each space has its own vec0 table so
//! KNN only compares vectors within the same model space.

use rusqlite::{Connection, params};
use zerocopy::IntoBytes;

use super::StorageError;
use super::crud::EntityType;

/// Which embedding table to operate on — determined by entity type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmbeddingSpace {
    Code,
    Knowledge,
}

impl EmbeddingSpace {
    pub fn table(&self) -> &'static str {
        match self {
            Self::Code => "code_embeddings",
            Self::Knowledge => "knowledge_embeddings",
        }
    }

    /// `vec_rowid.space` discriminator for this embedding space.
    fn name(&self) -> &'static str {
        match self {
            Self::Code => "code",
            Self::Knowledge => "knowledge",
        }
    }

    /// Determine the embedding space from an entity type string.
    /// Returns `None` for invalid types (caller should skip).
    pub fn from_entity_type(entity_type: &str) -> Option<Self> {
        EntityType::parse(entity_type).ok().map(|t| {
            if t.is_code() {
                Self::Code
            } else {
                Self::Knowledge
            }
        })
    }
}

/// Insert an embedding for an entity into the appropriate table.
/// The embedding dimension must match the vec0 table definition.
/// The table is selected based on the entity's type.
pub fn insert_embedding(
    conn: &Connection,
    entity_id: &str,
    entity_type: &str,
    embedding: &[f32],
) -> Result<(), StorageError> {
    let space = EmbeddingSpace::from_entity_type(entity_type)
        .ok_or_else(|| StorageError::InvalidEntityType(entity_type.to_string()))?;
    let sql = format!(
        "INSERT INTO {}(entity_id, embedding) VALUES (?1, ?2)",
        space.table()
    );
    conn.execute(&sql, params![entity_id, embedding.as_bytes()])?;
    conn.execute(
        "INSERT OR REPLACE INTO vec_rowid (entity_id, space, vec_rowid) VALUES (?1, ?2, ?3)",
        params![entity_id, space.name(), conn.last_insert_rowid()],
    )?;
    Ok(())
}

/// Delete an embedding from both tables (the entity's type may have
/// changed, so we check both). This is safe — only one table will
/// have the row. Rowids resolve through `vec_rowid` — a `WHERE
/// entity_id` on vec0 itself would be a full scan.
pub fn delete_embedding(conn: &Connection, entity_id: &str) -> Result<(), StorageError> {
    for space in [EmbeddingSpace::Code, EmbeddingSpace::Knowledge] {
        let sql = format!(
            "DELETE FROM {} WHERE rowid IN (\
             SELECT vec_rowid FROM vec_rowid WHERE entity_id = ?1 AND space = ?2)",
            space.table()
        );
        conn.execute(&sql, params![entity_id, space.name()])?;
    }
    conn.execute(
        "DELETE FROM vec_rowid WHERE entity_id = ?1",
        params![entity_id],
    )?;
    Ok(())
}

/// Get the embedding for a single entity. Checks both tables via
/// `vec_rowid` — vec0 is read by rowid, never scanned by entity_id.
/// Returns `None` if no embedding is stored for the entity.
/// Returns an error if the stored blob is corrupted (not a multiple of 4 bytes).
pub fn get_embedding(conn: &Connection, entity_id: &str) -> Result<Option<Vec<f32>>, StorageError> {
    // Try knowledge first (more common in query paths), then code.
    for space in [EmbeddingSpace::Knowledge, EmbeddingSpace::Code] {
        let sql = format!(
            "SELECT v.embedding FROM {} v \
             JOIN vec_rowid m ON m.vec_rowid = v.rowid \
             WHERE m.entity_id = ?1 AND m.space = ?2",
            space.table()
        );
        let mut stmt = conn.prepare(&sql)?;
        let result = stmt.query_row(params![entity_id, space.name()], |r| {
            let blob: Vec<u8> = r.get(0)?;
            if !blob.len().is_multiple_of(4) {
                return Err(rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "embedding blob is {} bytes, not a multiple of 4",
                            blob.len()
                        ),
                    )),
                ));
            }
            let floats: Vec<f32> = blob
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect();
            Ok(floats)
        });
        match result {
            Ok(embedding) => return Ok(Some(embedding)),
            Err(rusqlite::Error::QueryReturnedNoRows) => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(None)
}

/// Batch-fetch embeddings for multiple entity IDs from the knowledge
/// embeddings table. Returns a map of entity_id → embedding. Entities
/// without embeddings are simply absent from the map.
///
/// This is more efficient than calling `get_embedding` per entity when
/// you need embeddings for a large set of entities (e.g. all active
/// observations during merge candidate detection).
pub fn get_knowledge_embeddings_batch(
    conn: &Connection,
    entity_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<f32>>, StorageError> {
    get_embeddings_batch_from(conn, EmbeddingSpace::Knowledge, entity_ids)
}

/// Batch-fetch embeddings for multiple entity IDs from the code
/// embeddings table. Same contract as `get_knowledge_embeddings_batch`.
pub fn get_code_embeddings_batch(
    conn: &Connection,
    entity_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<f32>>, StorageError> {
    get_embeddings_batch_from(conn, EmbeddingSpace::Code, entity_ids)
}

/// Entity ids that should carry a vector but have none — the heal
/// set for embed passes, which otherwise only see freshly synced ids.
/// Covers entities whose embedding was never written (model absent at
/// index time, dropped migration rows, interrupted background embed).
/// Active-only: stale entities are excluded — stale-with-live-file
/// entities keep their existing vector, and stale tombstones whose
/// content is gone never get one.
pub fn ids_missing_embeddings(
    conn: &Connection,
    space: EmbeddingSpace,
) -> Result<Vec<String>, StorageError> {
    let types = match space {
        EmbeddingSpace::Code => "'function','class','file','module'",
        EmbeddingSpace::Knowledge => "'observation','rule','knowledge'",
    };
    let sql = format!(
        "SELECT id FROM entities \
         WHERE type IN ({types}) AND status = 'active' \
         AND id NOT IN (SELECT entity_id FROM vec_rowid WHERE space = '{}') \
         ORDER BY id",
        space.name()
    );
    let mut stmt = conn.prepare(&sql)?;
    let ids = stmt
        .query_map([], |r| r.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(ids)
}

fn get_embeddings_batch_from(
    conn: &Connection,
    space: EmbeddingSpace,
    entity_ids: &[String],
) -> Result<std::collections::HashMap<String, Vec<f32>>, StorageError> {
    if entity_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    const CHUNK_SIZE: usize = 998;
    let mut result = std::collections::HashMap::new();

    for chunk in entity_ids.chunks(CHUNK_SIZE) {
        if chunk.is_empty() {
            continue;
        }
        let placeholders = (0..chunk.len()).map(|_| "?").collect::<Vec<_>>().join(",");
        let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(space.name().to_string())];
        params.extend(
            chunk
                .iter()
                .map(|s| Box::new(s.clone()) as Box<dyn rusqlite::ToSql>),
        );
        let sql = format!(
            "SELECT m.entity_id, v.embedding \
             FROM vec_rowid m \
             JOIN {} v ON v.rowid = m.vec_rowid \
             WHERE m.space = ?1 AND m.entity_id IN ({placeholders})",
            space.table()
        );
        let mut stmt = conn.prepare(&sql)?;
        let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
        let rows = stmt.query_map(param_refs.as_slice(), |r| {
            let id: String = r.get(0)?;
            let blob: Vec<u8> = r.get(1)?;
            if !blob.len().is_multiple_of(4) {
                return Err(rusqlite::Error::FromSqlConversionFailure(
                    0,
                    rusqlite::types::Type::Blob,
                    Box::new(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "embedding blob is {} bytes, not a multiple of 4",
                            blob.len()
                        ),
                    )),
                ));
            }
            let floats: Vec<f32> = blob
                .as_chunks::<4>()
                .0
                .iter()
                .map(|chunk| f32::from_le_bytes(*chunk))
                .collect();
            Ok((id, floats))
        })?;
        for row in rows {
            let (id, embedding) = row?;
            result.insert(id, embedding);
        }
    }

    Ok(result)
}

/// Count total embeddings across both tables — `vec_rowid` mirrors
/// the vec0 rows one-to-one and is the cheaper count.
pub fn count_embeddings(conn: &Connection) -> Result<i64, StorageError> {
    let code: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vec_rowid WHERE space = 'code'",
        [],
        |r| r.get(0),
    )?;
    let knowledge: i64 = conn.query_row(
        "SELECT COUNT(*) FROM vec_rowid WHERE space = 'knowledge'",
        [],
        |r| r.get(0),
    )?;
    Ok(code + knowledge)
}

/// K-nearest-neighbor search within a specific embedding space.
/// Returns (entity_id, distance) pairs sorted by ascending distance.
pub fn knn_search(
    conn: &Connection,
    space: EmbeddingSpace,
    query: &[f32],
    k: i64,
) -> Result<Vec<(String, f32)>, StorageError> {
    let sql = format!(
        "SELECT entity_id, distance
         FROM {}
         WHERE embedding MATCH ?1 AND k = ?2
         ORDER BY distance",
        space.table()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![query.as_bytes(), k], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// KNN search filtered by entity type. Uses a subquery to restrict
/// results to entities of the specified type, preventing unrelated
/// entity types from consuming KNN slots. For example, when searching
/// for merge candidates among observations, this prevents knowledge
/// entries from crowding out observation neighbors.
pub fn knn_search_with_type_filter(
    conn: &Connection,
    space: EmbeddingSpace,
    query: &[f32],
    k: i64,
    entity_type: &str,
) -> Result<Vec<(String, f32)>, StorageError> {
    let sql = format!(
        "SELECT entity_id, distance
         FROM {}
         WHERE embedding MATCH ?1 AND k = ?2
           AND entity_id IN (SELECT id FROM entities WHERE type = ?3)
         ORDER BY distance",
        space.table()
    );
    let mut stmt = conn.prepare(&sql)?;
    let rows = stmt.query_map(params![query.as_bytes(), k, entity_type], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

/// KNN search filtered by entity type, status, and test-path exclusion.
/// Pushes all filters into the KNN subquery so non-matching entities
/// don't consume KNN slots. This prevents stale/rejected/superseded
/// embeddings and test entities from starving out active production
/// results in a bounded KNN window.
pub fn knn_search_with_filters(
    conn: &Connection,
    space: EmbeddingSpace,
    query: &[f32],
    k: i64,
    entity_type: Option<&str>,
    status: Option<&str>,
    exclude_tests: bool,
) -> Result<Vec<(String, f32)>, StorageError> {
    let mut where_clause = String::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![Box::new(query.as_bytes()), Box::new(k)];
    let mut next_idx = 3usize;

    if let Some(et) = entity_type {
        where_clause.push_str(&format!(
            " AND entity_id IN (SELECT id FROM entities WHERE type = ?{next_idx})"
        ));
        params.push(Box::new(et.to_string()));
        next_idx += 1;
    }
    if let Some(st) = status {
        where_clause.push_str(&format!(
            " AND entity_id IN (SELECT id FROM entities WHERE status = ?{next_idx})"
        ));
        params.push(Box::new(st.to_string()));
    }
    if exclude_tests {
        // is_test_path is the registered scalar wrapping
        // `index::gitignore::is_test_file` — the same predicate the
        // indexer and FTS path use. NULL file_path rows are never
        // test files, so is_test_path(NULL) = 0 keeps them.
        where_clause.push_str(
            " AND entity_id NOT IN (SELECT id FROM entities WHERE is_test_path(file_path) = 1)",
        );
    }

    let sql = format!(
        "SELECT entity_id, distance
         FROM {}
         WHERE embedding MATCH ?1 AND k = ?2{}
         ORDER BY distance",
        space.table(),
        where_clause
    );
    let mut stmt = conn.prepare(&sql)?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt.query_map(param_refs.as_slice(), |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f32>(1)?))
    })?;
    let mut results = Vec::new();
    for row in rows {
        results.push(row?);
    }
    Ok(results)
}

#[cfg(test)]
#[path = "embeddings_tests.rs"]
mod tests;
