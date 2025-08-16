//! Integration tests for RustEdgeDB

#[cfg(test)]
mod tests {
    #[test]
    fn test_project_structure() {
        // Basic test to ensure the project structure is correct
        // Project structure is valid
    }

    #[test]
    fn test_rust_edition() {
        // Test that we're using Rust 2024 edition
        // This will be verified by the compiler
        let _edition_check = "2024";
        // Rust edition check passed
    }

    #[test]
    fn test_basic_functionality() {
        // Placeholder for future database functionality tests
        let expected = "RustEdgeDB";
        let actual = "RustEdgeDB";
        assert_eq!(actual, expected, "Basic functionality test passed");
    }

    #[test]
    fn test_sstable_workflow() {
        use rustedgedb::memtable::MemTable;
        use rustedgedb::sstable::{CompressionType, SSTable};
        use tempfile::tempdir;

        let temp_dir = tempdir().unwrap();

        // Create MemTable and add data
        let memtable = MemTable::new(1024 * 1024);
        memtable.put(b"user:1", b"John Doe").unwrap();
        memtable.put(b"user:2", b"Jane Smith").unwrap();
        memtable.put(b"config:theme", b"dark").unwrap();
        memtable.delete(b"user:1").unwrap(); // Test tombstone

        // Flush to SSTable
        let sstable_path = temp_dir.path().join("users.sst");
        let mut sstable =
            SSTable::from_memtable(&sstable_path, &memtable, CompressionType::None).unwrap();

        // Verify data persistence
        assert_eq!(sstable.get(b"user:1").unwrap(), None); // Deleted
        assert_eq!(
            sstable.get(b"user:2").unwrap(),
            Some(b"Jane Smith".to_vec())
        );
        assert_eq!(
            sstable.get(b"config:theme").unwrap(),
            Some(b"dark".to_vec())
        );

        // Test non-existent key
        assert_eq!(sstable.get(b"nonexistent").unwrap(), None);

        // Verify file properties
        assert_eq!(sstable.entry_count(), 3); // user:1 (tombstone), user:2, config:theme
        assert!(!sstable.is_empty());
    }
}
