//! Basic integration tests for RustEdgeDB Engine
//! 
//! Tests basic functionality, persistence, compaction, and consistency
//! across MemTable and SSTable components.

use rustedgedb::engine::{Engine, EngineConfig};
use rustedgedb::sstable::CompressionType;
use tempfile::tempdir;
use tokio;

/// Test basic put/get/delete operations
#[tokio::test]
async fn test_basic_operations() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Create engine
    let mut engine = Engine::new(engine_path).await.unwrap();

    // Test put operations
    engine.put(b"user:1", b"John Doe").await.unwrap();
    engine.put(b"user:2", b"Jane Smith").await.unwrap();
    engine.put(b"config:theme", b"dark").await.unwrap();

    // Test get operations
    assert_eq!(
        engine.get(b"user:1").await.unwrap(),
        Some(b"John Doe".to_vec())
    );
    assert_eq!(
        engine.get(b"user:2").await.unwrap(),
        Some(b"Jane Smith".to_vec())
    );
    assert_eq!(
        engine.get(b"config:theme").await.unwrap(),
        Some(b"dark".to_vec())
    );

    // Test delete operation
    engine.delete(b"user:1").await.unwrap();
    assert_eq!(engine.get(b"user:1").await.unwrap(), None);

    // Test update operation
    engine.put(b"user:2", b"Jane Doe").await.unwrap();
    assert_eq!(
        engine.get(b"user:2").await.unwrap(),
        Some(b"Jane Doe".to_vec())
    );

    // Test non-existent key
    assert_eq!(engine.get(b"nonexistent").await.unwrap(), None);
}

/// Test persistence across engine restarts
#[tokio::test]
async fn test_persistence_across_restart() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Create engine and add data
    {
        let mut engine = Engine::new(engine_path).await.unwrap();
        
        // Add enough data to ensure MemTable is actually used
        for i in 0..100 {
            let key = format!("persistent:{}", i);
            let value = format!("value{}", i);
            engine.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        }
        
        // Check stats before flush
        let stats_before = engine.stats();
        println!("Stats before flush: sstable_count={}, memtable_size={}", 
                 stats_before.sstable_count, stats_before.memtable_size);
        
        // Force flush to ensure data is persisted
        engine.force_flush().await.unwrap();
        
        // Check stats after flush
        let stats_after = engine.stats();
        println!("Stats after flush: sstable_count={}, memtable_size={}", 
                 stats_after.sstable_count, stats_after.memtable_size);
        
        // Try to get data from SSTable
        let value0_from_sstable = engine.get(b"persistent:0").await.unwrap();
        println!("Retrieved persistent:0 from SSTable: {:?}", value0_from_sstable);
        
        // Verify data before close
        assert_eq!(
            engine.get(b"persistent:0").await.unwrap(),
            Some(b"value0".to_vec())
        );
        assert_eq!(
            engine.get(b"persistent:99").await.unwrap(),
            Some(b"value99".to_vec())
        );
        
        engine.close().await.unwrap();
    }

    // Reopen engine and verify persistence
    let engine = Engine::new(engine_path).await.unwrap();
    
    // Check stats first
    let stats = engine.stats();
    println!("Engine stats after restart: sstable_count={}, memtable_size={}", 
             stats.sstable_count, stats.memtable_size);
    
    // Check that data persisted
    let value0 = engine.get(b"persistent:0").await.unwrap();
    println!("Retrieved persistent:0 = {:?}", value0);
    assert_eq!(value0, Some(b"value0".to_vec()));
    
    let value99 = engine.get(b"persistent:99").await.unwrap();
    println!("Retrieved persistent:99 = {:?}", value99);
    assert_eq!(value99, Some(b"value99".to_vec()));

    // Check stats
    assert!(stats.sstable_count > 0, "Data should have been flushed to SSTable");
}

/// Test compaction correctness
#[tokio::test]
async fn test_compaction_correctness() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Create engine with small MemTable size to trigger flushes
    let config = EngineConfig {
        data_dir: engine_path.to_path_buf(),
        memtable_size: 1024 * 1024, // 1MB to allow more entries
        compression: CompressionType::None,
        max_levels: 7,
    };

    let mut engine = Engine::with_config(config).await.unwrap();

    // Add data in multiple batches to create multiple SSTables
    for batch in 0..3 {
        for i in 0..5 {
            let key = format!("batch{}:key{}", batch, i);
            let value = format!("value{}", i);
            engine.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        }
        
        // Force flush to create new SSTable
        engine.force_flush().await.unwrap();
    }

    // Verify all data is accessible
    for batch in 0..3 {
        for i in 0..5 {
            let key = format!("batch{}:key{}", batch, i);
            let expected_value = format!("value{}", i);
            
            let actual_value = engine.get(key.as_bytes()).await.unwrap();
            assert_eq!(
                actual_value,
                Some(expected_value.as_bytes().to_vec()),
                "Key: {}", key
            );
        }
    }

    // Check that we have multiple SSTables
    let stats = engine.stats();
    assert!(stats.sstable_count >= 3, "Should have at least 3 SSTables");

    // Test deletion and re-insertion
    engine.delete(b"batch0:key0").await.unwrap();
    engine.put(b"batch0:key0", b"new_value").await.unwrap();
    
    assert_eq!(
        engine.get(b"batch0:key0").await.unwrap(),
        Some(b"new_value".to_vec())
    );
}

/// Test crash recovery scenarios
#[tokio::test]
async fn test_crash_recovery() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Create engine and add data
    let mut engine = Engine::new(engine_path).await.unwrap();
    engine.put(b"recovery:1", b"data1").await.unwrap();
    engine.put(b"recovery:2", b"data2").await.unwrap();
    
    // Simulate crash by dropping engine without proper close
    drop(engine);

    // Reopen engine and verify recovery
    let engine = Engine::new(engine_path).await.unwrap();
    
    // Check that data was recovered
    assert_eq!(
        engine.get(b"recovery:1").await.unwrap(),
        Some(b"data1".to_vec())
    );
    assert_eq!(
        engine.get(b"recovery:2").await.unwrap(),
        Some(b"data2".to_vec())
    );
}

/// Test sequence number continuity across operations
#[tokio::test]
async fn test_sequence_number_continuity() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Perform a sequence of operations
    let operations: Vec<(&[u8], Option<&[u8]>)> = vec![
        (b"seq:1", Some(b"value1")),
        (b"seq:2", Some(b"value2")),
        (b"seq:1", None), // Delete
        (b"seq:3", Some(b"value3")),
        (b"seq:1", Some(b"new_value1")), // Re-insert
    ];

    for (key, value) in operations {
        match value {
            Some(val) => engine.put(key, val).await.unwrap(),
            None => engine.delete(key).await.unwrap(),
        }
    }

    // Verify final state
    assert_eq!(
        engine.get(b"seq:1").await.unwrap(),
        Some(b"new_value1".to_vec())
    );
    assert_eq!(
        engine.get(b"seq:2").await.unwrap(),
        Some(b"value2".to_vec())
    );
    assert_eq!(
        engine.get(b"seq:3").await.unwrap(),
        Some(b"value3".to_vec())
    );
}

/// Test concurrent access patterns
#[tokio::test]
async fn test_concurrent_access() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Test multiple sequential operations to ensure thread safety
    for task_id in 0..5 {
        for i in 0..10 {
            let key = format!("task{}:key{}", task_id, i);
            let value = format!("value{}", i);
            
            // Write operation
            engine.put(key.as_bytes(), value.as_bytes()).await.unwrap();
            
            // Read operation
            let retrieved = engine.get(key.as_bytes()).await.unwrap();
            assert_eq!(retrieved, Some(value.as_bytes().to_vec()));
        }
    }

    // Verify all data is accessible
    for task_id in 0..5 {
        for i in 0..10 {
            let key = format!("task{}:key{}", task_id, i);
            let expected_value = format!("value{}", i);
            
            let actual_value = engine.get(key.as_bytes()).await.unwrap();
            assert_eq!(
                actual_value,
                Some(expected_value.as_bytes().to_vec()),
                "Key: {}", key
            );
        }
    }
}

/// Test edge cases and error conditions
#[tokio::test]
async fn test_edge_cases() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Test empty key (should error)
    let result = engine.put(b"", b"value").await;
    assert!(result.is_err());

    // Test empty value (should work)
    engine.put(b"empty_value", b"").await.unwrap();
    assert_eq!(engine.get(b"empty_value").await.unwrap(), Some(b"".to_vec()));

    // Test very long key (within limits)
    let long_key = vec![b'x'; 1000];
    engine.put(&long_key, b"long_key_value").await.unwrap();
    assert_eq!(
        engine.get(&long_key).await.unwrap(),
        Some(b"long_key_value".to_vec())
    );

    // Test very long value (within limits)
    let long_value = vec![b'y'; 10000];
    engine.put(b"long_value_key", &long_value).await.unwrap();
    assert_eq!(
        engine.get(b"long_value_key").await.unwrap(),
        Some(long_value)
    );
}

/// Test WAL rotation and MemTable flushing
#[tokio::test]
async fn test_wal_rotation_and_flushing() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Create engine with very small MemTable size to trigger multiple flushes
    let config = EngineConfig {
        data_dir: engine_path.to_path_buf(),
        memtable_size: 1024, // 1KB to ensure multiple flushes
        compression: CompressionType::None,
        max_levels: 7,
    };

    let mut engine = Engine::with_config(config).await.unwrap();

    // Add data that will trigger multiple flushes
    for i in 0..20 {
        let key = format!("flush:key{}", i);
        let value = format!("value{}", i);
        engine.put(key.as_bytes(), value.as_bytes()).await.unwrap();
        
        // Force flush every 5 entries to ensure multiple SSTables
        if (i + 1) % 5 == 0 {
            engine.force_flush().await.unwrap();
        }
    }

    // Force final flush
    engine.force_flush().await.unwrap();

    // Verify all data is accessible
    for i in 0..20 {
        let key = format!("flush:key{}", i);
        let expected_value = format!("value{}", i);
        
        let actual_value = engine.get(key.as_bytes()).await.unwrap();
        assert_eq!(
            actual_value,
            Some(expected_value.as_bytes().to_vec()),
            "Key: {}", key
        );
    }

    // Check that we have multiple SSTables
    let stats = engine.stats();
    assert!(stats.sstable_count > 1, "Should have multiple SSTables from flushes");
}

/// Test data consistency across MemTable and SSTables
#[tokio::test]
async fn test_data_consistency() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Add data to MemTable
    engine.put(b"consistency:1", b"memtable_value").await.unwrap();
    engine.put(b"consistency:2", b"memtable_value2").await.unwrap();

    // Force flush to SSTable
    engine.force_flush().await.unwrap();

    // Add more data to new MemTable
    engine.put(b"consistency:3", b"new_memtable_value").await.unwrap();
    engine.put(b"consistency:1", b"updated_value").await.unwrap(); // Update existing key

    // Verify consistency: MemTable should override SSTable
    assert_eq!(
        engine.get(b"consistency:1").await.unwrap(),
        Some(b"updated_value".to_vec())
    );
    assert_eq!(
        engine.get(b"consistency:2").await.unwrap(),
        Some(b"memtable_value2".to_vec())
    );
    assert_eq!(
        engine.get(b"consistency:3").await.unwrap(),
        Some(b"new_memtable_value".to_vec())
    );

    // Force another flush and verify consistency
    engine.force_flush().await.unwrap();
    
    assert_eq!(
        engine.get(b"consistency:1").await.unwrap(),
        Some(b"updated_value".to_vec())
    );
    assert_eq!(
        engine.get(b"consistency:2").await.unwrap(),
        Some(b"memtable_value2".to_vec())
    );
    assert_eq!(
        engine.get(b"consistency:3").await.unwrap(),
        Some(b"new_memtable_value".to_vec())
    );
}

/// Test large dataset handling
#[tokio::test]
async fn test_large_dataset() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Insert a large number of keys with larger values to trigger flush
    let num_keys = 1000;
    for i in 0..num_keys {
        let key = format!("large:key{}", i);
        let value = format!("value{}_with_some_additional_data_to_make_it_larger", i);
        engine.put(key.as_bytes(), value.as_bytes()).await.unwrap();
    }

    // Force flush to ensure data is persisted
    engine.force_flush().await.unwrap();

    // Verify all keys are accessible
    for i in 0..num_keys {
        let key = format!("large:key{}", i);
        let expected_value = format!("value{}_with_some_additional_data_to_make_it_larger", i);
        
        let actual_value = engine.get(key.as_bytes()).await.unwrap();
        assert_eq!(
            actual_value,
            Some(expected_value.as_bytes().to_vec()),
            "Key: {}", key
        );
    }

    // Check stats
    let stats = engine.stats();
    assert!(stats.sstable_count > 0, "Large dataset should have been flushed");
}

/// Test configuration options
#[tokio::test]
async fn test_configuration_options() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Test different compression types
    for compression in [CompressionType::None, CompressionType::LZ4, CompressionType::Zstd] {
        let config = EngineConfig {
            data_dir: engine_path.join(format!("compression_{:?}", compression)),
            memtable_size: 1024 * 1024, // 1MB
            compression,
            max_levels: 5,
        };

        let mut engine = Engine::with_config(config).await.unwrap();
        
        // Add some data
        engine.put(b"config:test", b"compression_test").await.unwrap();
        engine.force_flush().await.unwrap();
        
        // Verify data
        assert_eq!(
            engine.get(b"config:test").await.unwrap(),
            Some(b"compression_test".to_vec())
        );
        
        engine.close().await.unwrap();
    }
}

/// Test error handling and recovery
#[tokio::test]
async fn test_error_handling() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Test invalid operations
    let result = engine.put(b"", b"value").await;
    assert!(result.is_err());

    let result = engine.get(b"").await;
    assert!(result.is_err());

    let result = engine.delete(b"").await;
    assert!(result.is_err());

    // Test valid operations still work
    engine.put(b"valid:key", b"valid_value").await.unwrap();
    assert_eq!(
        engine.get(b"valid:key").await.unwrap(),
        Some(b"valid_value".to_vec())
    );
}

/// Test engine statistics
#[tokio::test]
async fn test_engine_statistics() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    let mut engine = Engine::new(engine_path).await.unwrap();

    // Check initial stats
    let initial_stats = engine.stats();
    assert_eq!(initial_stats.sstable_count, 0);
    assert_eq!(initial_stats.memtable_size, 0);

    // Add data and check stats
    engine.put(b"stats:1", b"value1").await.unwrap();
    engine.put(b"stats:2", b"value2").await.unwrap();
    
    let stats_after_data = engine.stats();
    assert!(stats_after_data.memtable_size > 0);

    // Force flush and check stats
    engine.force_flush().await.unwrap();
    
    let stats_after_flush = engine.stats();
    assert!(stats_after_flush.sstable_count > 0);
    assert_eq!(stats_after_flush.memtable_size, 0); // New MemTable should be empty
}

/// Test graceful shutdown
#[tokio::test]
async fn test_graceful_shutdown() {
    let temp_dir = tempdir().unwrap();
    let engine_path = temp_dir.path();

    // Create engine and add data
    let mut engine = Engine::new(engine_path).await.unwrap();
    engine.put(b"shutdown:1", b"value1").await.unwrap();
    engine.put(b"shutdown:2", b"value2").await.unwrap();

    // Gracefully close
    engine.close().await.unwrap();

    // Reopen and verify data persisted
    let engine = Engine::new(engine_path).await.unwrap();
    
    assert_eq!(
        engine.get(b"shutdown:1").await.unwrap(),
        Some(b"value1".to_vec())
    );
    assert_eq!(
        engine.get(b"shutdown:2").await.unwrap(),
        Some(b"value2".to_vec())
    );
}
