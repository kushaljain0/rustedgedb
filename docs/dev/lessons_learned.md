# Lessons Learned - RustEdgeDB Development

## Table of Contents
1. [MemTable Implementation](#memtable-implementation)
2. [WAL Implementation](#wal-implementation)
3. [SSTable Implementation](#sstable-implementation)
4. [Compaction Implementation](#compaction-implementation)
5. [Common Rust Patterns](#common-rust-patterns)
6. [Thread Safety Patterns](#thread-safety-patterns)
7. [Testing Best Practices](#testing-best-practices)
8. [Documentation Workflow](#documentation-workflow)

---

## MemTable Implementation

### **Data Structure Choice: Sorted Vector vs Complex Structures**

#### Problem Encountered
Initially attempted to implement a skiplist with `Arc<SkipListNode>` for the MemTable, but encountered multiple borrowing and mutability issues.

#### Root Cause
- `Arc` provides **immutable shared access** only
- Complex pointer-based data structures are difficult to make thread-safe in Rust
- Multiple mutable references create complex ownership patterns

#### Solution Implemented
Switched to a **sorted vector with binary search** approach:
```rust
pub struct MemTable {
    data: Arc<RwLock<Vec<Entry>>>,
    // ... other fields
}
```

#### Key Learnings
1. **Start Simple**: Begin with the simplest working solution, then optimize
2. **Thread Safety First**: Design for thread safety from the beginning
3. **Performance Trade-offs**: O(log n) reads vs O(n) inserts is acceptable for MemTable use case
4. **Maintainability**: Simple, correct code beats complex, buggy code

#### Performance Characteristics
- **Read Operations**: O(log n) with binary search
- **Write Operations**: O(n) due to vector shifting
- **Memory Usage**: Efficient for small to medium datasets
- **Thread Safety**: Excellent with Arc + RwLock

---

## WAL Implementation

### **File I/O and Buffering Strategy**

#### Problem Encountered
Initially considered using raw `File` operations for WAL writes, but this would require manual buffering and could lead to performance issues.

#### Root Cause
- **Raw file I/O**: Each write operation is a system call
- **Manual buffering**: Complex to implement correctly
- **Performance**: Multiple small writes become expensive
- **Durability**: Need to ensure data is actually written to disk

#### Solution Implemented
Used `BufWriter<File>` for automatic buffering:
```rust
pub struct WAL {
    file: BufWriter<File>,
    path: PathBuf,
    sequence_number: u64,
}
```

#### Key Learnings
1. **Use standard library buffering**: `BufWriter` provides efficient buffering automatically
2. **Explicit flush calls**: Call `flush()` after critical writes for durability
3. **File path management**: Store `PathBuf` for recovery operations
4. **Sequence number tracking**: Maintain sequence numbers for crash recovery

#### Performance Characteristics
- **Write Operations**: Buffered for efficiency, explicit flush for durability
- **Recovery**: O(n) where n is number of records
- **Memory Usage**: Minimal overhead with BufWriter
- **Durability**: Guaranteed with explicit flush calls

---

### **Binary Record Format Design**

#### Problem Encountered
Need to design a binary format that's both efficient and recoverable from corruption.

#### Root Cause
- **Variable-length data**: Keys and values can be any size
- **Corruption resilience**: Must handle partial writes gracefully
- **Recovery**: Need to find next valid record after corruption
- **Validation**: Must detect invalid record sizes

#### Solution Implemented
Fixed-size header with variable-length data:
```rust
// Header: 24 bytes total
[Key Length: 4 bytes (u32, little-endian)]
[Value Length: 4 bytes (u32, little-endian)] 
[Timestamp: 8 bytes (u64, little-endian)]
[Sequence Number: 8 bytes (u64, little-endian)]
[Key Data: variable length]
[Value Data: variable length (if value_len > 0)]
```

#### Key Learnings
1. **Fixed-size headers**: Make recovery and seeking easier
2. **Little-endian encoding**: Consistent with most modern systems
3. **Size validation**: Check key/value lengths to prevent abuse
4. **Corruption detection**: Use size limits to identify invalid records

#### Recovery Strategy
```rust
fn seek_to_next_record<R: Read + Seek>(&self, reader: &mut R) -> WALResult<()> {
    // Look for potential record headers with reasonable lengths
    // Seek to next valid record start
    // Continue recovery from there
}
```

---

### **Error Handling and Recovery**

#### Problem Encountered
Need to handle various failure modes gracefully without losing data.

#### Root Cause
- **Partial writes**: System crashes during write operations
- **File corruption**: Disk errors or incomplete writes
- **Invalid data**: Malformed records or size mismatches
- **Recovery complexity**: Must continue from corruption points

#### Solution Implemented
Comprehensive error handling with recovery mechanisms:
```rust
#[derive(Error, Debug)]
pub enum WALError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("Invalid record format: {0}")]
    InvalidRecord(String),
    #[error("Corrupted WAL file: {0}")]
    CorruptedFile(String),
    #[error("MemTable error: {0}")]
    MemTable(#[from] MemTableError),
}
```

#### Key Learnings
1. **Use thiserror**: Provides excellent error context and conversion
2. **Recoverable vs fatal**: Distinguish between recoverable and fatal errors
3. **Graceful degradation**: Continue recovery from corruption points
4. **Comprehensive logging**: Log all recovery attempts for debugging

---

### **Sequence Number Management**

#### Problem Encountered
Need to maintain sequence numbers across WAL restarts and ensure continuity.

#### Root Cause
- **Crash recovery**: Must recover sequence numbers from existing WAL
- **Continuity**: Sequence numbers must be monotonically increasing
- **Validation**: Must detect sequence number mismatches
- **Recovery**: Need to find highest sequence number in corrupted files

#### Solution Implemented
Automatic sequence number recovery on startup:
```rust
fn recover_sequence_number(&mut self) -> WALResult<()> {
    // Read existing WAL file to find highest sequence number
    // Continue from that point for new operations
    // Handle corruption gracefully
}
```

#### Key Learnings
1. **Recover on startup**: Always scan existing WAL for sequence numbers
2. **Handle corruption**: Stop at first corruption, use highest valid sequence
3. **Validation**: Check sequence number continuity in write operations
4. **Atomic updates**: Update sequence number only after successful write

---

### **Testing WAL Durability and Recovery**

#### Problem Encountered
Need to test crash recovery scenarios and corruption handling.

#### Root Cause
- **Crash simulation**: Difficult to simulate real system crashes
- **Corruption testing**: Need to test various corruption scenarios
- **Recovery validation**: Must ensure data integrity after recovery
- **Edge cases**: Test boundary conditions and error scenarios

#### Solution Implemented
Comprehensive test suite with corruption simulation:
```rust
#[test]
fn test_wal_corruption_handling() {
    // Write valid records
    // Corrupt file by appending garbage
    // Test recovery succeeds despite corruption
    // Verify valid records are recovered correctly
}
```

#### Key Learnings
1. **Test corruption scenarios**: Append garbage data to test recovery
2. **Validate recovery**: Ensure all valid data is recovered
3. **Test edge cases**: Empty files, single records, large records
4. **Use tempfile**: Temporary directories for isolated testing

---

## SSTable Implementation

### **File Format Design and Validation**

#### Problem Encountered
Need to design a binary file format that's both efficient and recoverable, with proper header/footer validation.

#### Root Cause
- **Complex file structure**: Multiple sections (header, bloom filter, data, index, footer)
- **Offset management**: Need to calculate and update offsets during writing
- **Validation**: Must detect corrupted or invalid files
- **Recovery**: Need to handle partial writes and corruption

#### Solution Implemented
Structured file format with validation:
```rust
pub struct SSTableHeader {
    magic: [u8; 8],           // "RUSTEDGE" magic number
    version: u32,              // File format version
    entry_count: u32,          // Number of key-value pairs
    index_offset: u64,         // Offset to index section
    bloom_filter_offset: u64,  // Offset to bloom filter
    data_offset: u64,          // Offset to data section
    compression_type: u8,      // Compression algorithm
    reserved: [u8; 31],       // Reserved for future use
}
```

#### Key Learnings
1. **Magic numbers**: Use distinctive magic numbers for file identification
2. **Versioning**: Include version field for future format compatibility
3. **Offset management**: Calculate offsets during writing, update header at end
4. **Validation**: Check magic numbers and reasonable bounds on startup

---

## Compaction Implementation

### **Multi-SSTable Merging Strategy**

#### Problem Encountered
Need to merge multiple SSTables while maintaining sorted order, removing tombstones, and keeping only the most recent values.

#### Root Cause
- **Complex merging**: Multiple input files with overlapping key ranges
- **Tombstone handling**: Deleted keys must be removed during compaction
- **Duplicate resolution**: Same key may exist in multiple SSTables
- **Sorting**: Output must maintain sorted key order
- **Memory efficiency**: Can't load all data into memory at once

#### Solution Implemented
Two-phase compaction approach:
```rust
pub fn compact_sstables<P: AsRef<Path>>(&self, input_paths: &[P]) -> CompactionResult<PathBuf> {
    // Phase 1: Collect all entries from input SSTables
    let mut all_entries = Vec::new();
    for (i, path) in input_paths.iter().enumerate() {
        // Read entries and add source SSTable index
    }
    
    // Phase 2: Remove tombstones and duplicates
    let final_entries = self.remove_tombstones_and_duplicates(all_entries)?;
    
    // Phase 3: Write compacted output
    self.write_compacted_sstable(&mut writer, final_entries)?;
}
```

#### Key Learnings
1. **Source tracking**: Track which SSTable each entry came from for debugging
2. **Two-phase approach**: Separate collection from processing for clarity
3. **Tombstone removal**: Remove deletions during compaction to reclaim space
4. **Duplicate resolution**: Keep highest sequence number for each key

### **Tombstone and Duplicate Removal**

#### Problem Encountered
Need efficient algorithm to remove tombstones and keep only the most recent value for each key.

#### Root Cause
- **Sorting complexity**: Must sort by key, then by sequence number
- **Memory usage**: Storing all entries can be memory-intensive
- **Algorithm efficiency**: O(n log n) sorting for large datasets
- **Edge cases**: Handle empty inputs, single entries, all tombstones

#### Solution Implemented
Sort-based deduplication:
```rust
fn remove_tombstones_and_duplicates(
    &self,
    mut entries: Vec<CompactionEntry>,
) -> CompactionResult<Vec<CompactionEntry>> {
    // Sort by key, then by sequence number (descending)
    entries.sort_by(|a, b| {
        a.key.cmp(&b.key)
            .then_with(|| b.sequence_number.cmp(&a.sequence_number))
    });
    
    let mut final_entries = Vec::new();
    let mut current_key: Option<Vec<u8>> = None;
    
    for entry in entries {
        if current_key != Some(entry.key.clone()) {
            // New key, add it if it's not a tombstone
            if !entry.is_deletion() {
                final_entries.push(entry.clone());
            }
            current_key = Some(entry.key);
        }
        // Skip duplicates (we already have the most recent)
    }
    
    Ok(final_entries)
}
```

#### Key Learnings
1. **Sorting strategy**: Sort by key first, then by sequence number descending
2. **Single pass**: Process entries in sorted order to avoid multiple iterations
3. **Tombstone handling**: Only add non-tombstone entries to final result
4. **Memory efficiency**: Process in single pass rather than multiple collections

### **File Writing and Offset Management**

#### Problem Encountered
Need to write compacted data while managing file offsets for header, bloom filter, data, and index sections.

#### Root Cause
- **Unknown sizes**: Can't know final sizes until all data is written
- **Offset dependencies**: Index and bloom filter offsets depend on data size
- **File positioning**: Need to seek back and forth to update headers
- **Atomicity**: File must be consistent even if writing fails

#### Solution Implemented
Placeholder-based writing with final updates:
```rust
fn write_compacted_sstable(
    &self,
    writer: &mut BufWriter<File>,
    entries: Vec<CompactionEntry>,
) -> CompactionResult<()> {
    // Write header placeholder
    let header_placeholder = vec![0u8; header_size];
    writer.write_all(&header_placeholder)?;
    
    // Write bloom filter placeholder
    let bloom_filter_offset = writer.stream_position()?;
    let bloom_filter_placeholder = vec![0u8; bloom_filter_size];
    writer.write_all(&bloom_filter_placeholder)?;
    
    // Write data section
    let data_offset = writer.stream_position()?;
    // ... write entries ...
    
    // Write index section
    let index_offset = writer.stream_position()?;
    // ... write index ...
    
    // Update bloom filter and header with final offsets
    writer.seek(SeekFrom::Start(bloom_filter_offset))?;
    writer.write_all(bloom_filter.bits())?;
    
    writer.seek(SeekFrom::Start(0))?;
    header.write(writer)?;
}
```

#### Key Learnings
1. **Placeholder approach**: Write placeholders first, update with real data later
2. **Offset tracking**: Use `stream_position()` to track current file position
3. **Seek operations**: Seek back to update headers and metadata
4. **Atomic updates**: Update all metadata at the end for consistency

### **Testing Compaction Correctness**

#### Problem Encountered
Need comprehensive tests to verify compaction behavior: tombstone removal, duplicate resolution, sorting, and file format.

#### Root Cause
- **Complex behavior**: Multiple input files with various scenarios
- **Edge cases**: Empty inputs, all tombstones, duplicate keys
- **File validation**: Must verify output SSTable is valid and readable
- **Performance**: Tests should run quickly for development feedback

#### Solution Implemented
Comprehensive test suite with realistic scenarios:
```rust
#[test]
fn test_compaction_removes_tombstones() {
    // Test that deleted keys are removed during compaction
}

#[test]
fn test_compaction_guarantees_sorted_order() {
    // Test that output maintains sorted key order
}

#[test]
fn test_compaction_keeps_most_recent_value() {
    // Test that highest sequence number wins
}

#[test]
fn test_compaction_creates_valid_sstable() {
    // Test that output can be opened and read
}
```

#### Key Learnings
1. **Realistic test data**: Use actual MemTable data rather than mock data
2. **Multiple scenarios**: Test various input combinations and edge cases
3. **Output validation**: Verify that compacted SSTable is valid and readable
4. **Performance testing**: Ensure tests run quickly for development workflow

---

## Common Rust Patterns

### **Arc Mutability Issues**

#### Problem
```rust
// This won't work - Arc is immutable
let node = Arc::new(SkipListNode::new(entry));
node.next[0] = Some(other_node); // Error: cannot borrow as mutable
```

#### Solution
```rust
// Use Arc<RwLock<T>> for shared mutable data
let data = Arc::new(RwLock::new(Vec::new()));
let mut data = data.write().unwrap();
data.push(entry);
```

#### Learning
- `Arc<T>` = shared immutable data
- `Arc<RwLock<T>>` = shared mutable data
- `Arc<Mutex<T>>` = shared mutable data with different locking semantics

### **Type Mismatches in Comparisons**

#### Problem
```rust
// Vec<u8> vs &[u8] comparison
data.binary_search_by(|e| e.key.cmp(&entry.key)) // Type mismatch
```

#### Solution
```rust
// Convert both to &[u8] for comparison
data.binary_search_by(|e| e.key.as_slice().cmp(entry.key.as_slice()))
```

#### Learning
- `Vec<u8>` and `&[u8]` are different types
- Use `.as_slice()` to convert `Vec<u8>` to `&[u8]`
- Rust's type system catches these at compile time

### **Clippy Optimizations**

#### Problem
```rust
// Inefficient pattern
.map(|entry| entry.value.clone()).flatten()
```

#### Solution
```rust
// More efficient
.and_then(|entry| entry.value.clone())
```

#### Learning
- `map().flatten()` can often be replaced with `and_then()`
- Clippy catches these inefficiencies
- Following clippy suggestions leads to better code

### **Compilation and Type System Challenges**

#### Problem
```rust
// Type mismatch in arithmetic operations
let mut offset = 0; // usize
offset += n; // n is usize, but offset needs to be i64 for seek operations
```

#### Solution
```rust
// Explicit type annotation and conversion
let mut offset: i64 = 0;
offset += n as i64;
```

#### Learning
- **Explicit types**: Use type annotations when Rust can't infer the right type
- **Type conversions**: Use `as` for safe numeric conversions
- **Seek operations**: File seeking requires `i64` for relative positioning

#### Move Semantics with File Handles

#### Problem
```rust
// Can't move out of mutable reference
drop(self.file); // Error: cannot move out of `self.file`
```

#### Solution
```rust
// Reassign instead of moving
self.file = BufWriter::new(new_file);
```

#### Learning
- **File handle management**: Reassign file handles rather than moving them
- **Resource cleanup**: Let Rust's drop system handle cleanup automatically
- **Mutable references**: Be careful with move operations on borrowed data

---

## Thread Safety Patterns

### **Arc + RwLock Pattern**

#### When to Use
- **Shared data** accessed by multiple threads
- **Read-heavy workloads** (RwLock allows multiple readers)
- **Occasional writes** that can wait for readers to finish

#### Implementation
```rust
pub struct ThreadSafeStruct {
    data: Arc<RwLock<Vec<Entry>>>,
}

impl ThreadSafeStruct {
    pub fn read_operation(&self) -> Result<Vec<Entry>, Error> {
        let data = self.data.read().unwrap();
        Ok(data.clone())
    }
    
    pub fn write_operation(&self, entry: Entry) -> Result<(), Error> {
        let mut data = self.data.write().unwrap();
        data.push(entry);
        Ok(())
    }
}
```

#### Best Practices
1. **Minimize lock scope**: Hold locks for shortest time possible
2. **Avoid nested locks**: Can lead to deadlocks
3. **Use appropriate lock type**: RwLock for read-heavy, Mutex for write-heavy
4. **Handle lock failures**: Always handle `unwrap()` or use `?` operator

---

## Testing Best Practices

### **Realistic Test Data**

#### Problem
```rust
// Test that doesn't actually trigger the condition
let memtable = MemTable::new(100);
memtable.put(b"key1", b"value1").unwrap(); // 26 bytes
memtable.put(b"key2", b"value2").unwrap(); // 52 bytes
assert!(memtable.is_full()); // Fails: 52 < 100
```

#### Solution
```rust
// Test with data that actually triggers the condition
let memtable = MemTable::new(50); // Limit that allows 1 entry but not 2
memtable.put(b"key1", b"value1").unwrap();
let result = memtable.put(b"key2", b"value2");
assert!(result.is_err()); // Now this actually tests the limit
```

#### Learning
- **Test with realistic data** that triggers the conditions
- **Verify assumptions** about data sizes and limits
- **Test edge cases** and boundary conditions
- **Use appropriate assertions** for the actual behavior

### **Comprehensive Test Coverage**

#### What to Test
1. **Happy path**: Normal operations work correctly
2. **Edge cases**: Boundary conditions and limits
3. **Error conditions**: Invalid inputs and error handling
4. **Thread safety**: Concurrent access patterns
5. **Performance**: Size limits and memory management

#### Test Organization
```rust
#[cfg(test)]
mod tests {
    #[test]
    fn test_basic_operations() { /* ... */ }
    
    #[test]
    fn test_edge_cases() { /* ... */ }
    
    #[test]
    fn test_error_handling() { /* ... */ }
    
    #[test]
    fn test_thread_safety() { /* ... */ }
}
```

---

## Documentation Workflow

### **Documentation First Principle**

#### What Should Happen
1. **Update specification** before implementation
2. **Define API contracts** and data structures
3. **Document design decisions** and trade-offs
4. **Update implementation** to match documentation

#### What Actually Happened (Mistake)
1. ❌ Implemented MemTable first
2. ❌ Then updated documentation to match
3. ✅ Should have been the reverse

#### What Happened with WAL (Correct)
1. ✅ Updated specification first with WAL design
2. ✅ Implemented WAL according to specification
3. ✅ Updated documentation to reflect actual implementation details
4. ✅ Maintained consistency between spec and code

#### Correct Workflow
```bash
# 1. Update specification
git add docs/spec.md
git commit -m "docs(spec): update MemTable design to use sorted vector"

# 2. Implement feature
# ... implementation code ...

# 3. Ensure consistency
# ... tests and validation ...
```

#### Learning
- **Always update documentation first**
- **Documentation serves as specification**
- **Implementation should follow documented design**
- **Maintain consistency between docs and code**

---

## Common Pitfalls to Avoid

### **1. Over-Engineering**
- **Don't**: Start with complex data structures
- **Do**: Start simple and optimize when needed

### **2. Ignoring Thread Safety**
- **Don't**: Implement single-threaded first, then add thread safety
- **Do**: Design for thread safety from the beginning

### **3. Testing Assumptions**
- **Don't**: Test with data that doesn't trigger conditions
- **Do**: Test with realistic data that exercises the code paths

### **4. Documentation Afterthought**
- **Don't**: Implement first, document later
- **Do**: Document design decisions before implementation

### **5. Ignoring Rust Tools**
- **Don't**: Ignore clippy warnings and rustc errors
- **Do**: Use them to write better, more idiomatic code

### **6. Complex File I/O Patterns**
- **Don't**: Use raw file operations without proper buffering
- **Do**: Use `BufWriter` for automatic buffering and explicit flush for durability

### **7. Inadequate Corruption Recovery**
- **Don't**: Fail fast on any corruption
- **Do**: Implement graceful recovery mechanisms that can continue from corruption points

---

## Engine Implementation

### **Documentation-First Workflow Violation**

#### Problem Encountered
Initially implemented the Engine without updating the specification first, violating the project's "Documentation First" principle.

#### Root Cause
- **Rushed implementation**: Started coding before updating `docs/spec.md`
- **Workflow violation**: Did not follow the spec-driven development process
- **Context loss**: Implementation details not documented for future reference

#### Solution Implemented
1. **Retroactive documentation**: Updated `docs/spec.md` with complete Engine specification
2. **API documentation**: Documented all public methods and coordination patterns
3. **Invariants documentation**: Added Engine-specific guarantees and verification strategies
4. **Testing strategy**: Documented test coverage and patterns

#### Key Learnings
1. **Always update documentation first**: Follow the "Documentation First" principle
2. **Spec-driven development**: Implementation should follow documented specification
3. **Workflow compliance**: Adhere to project development rules even when excited to code
4. **Retroactive documentation**: Better late than never, but avoid this pattern

#### Prevention
- **Check cursor rules** before starting any implementation
- **Update specification** before writing any code
- **Follow established workflow** even for "simple" features
- **Document design decisions** as they're made, not after

---

### **Async Constructor Pattern**

#### Problem Encountered
Need to make Engine constructors async to support proper initialization and recovery.

#### Root Cause
- **Recovery operations**: WAL recovery requires async file operations
- **Test compatibility**: Tests need to await Engine creation
- **Initialization complexity**: Engine setup involves multiple async operations

#### Solution Implemented
```rust
// Make constructors async
pub async fn new<P: AsRef<Path>>(data_dir: P) -> EngineResult<Self>
pub async fn with_config(config: EngineConfig) -> EngineResult<Self>

// Update all tests to await creation
let engine = Engine::new(temp_dir.path()).await.unwrap();
```

#### Key Learnings
1. **Async constructors**: Can be necessary for complex initialization
2. **Test compatibility**: All tests must await async constructors
3. **Initialization order**: Recovery operations must complete before Engine is usable
4. **Error handling**: Constructor errors must be properly propagated

#### Prevention
- **Design async patterns** from the beginning for complex initialization
- **Consider test patterns** when designing public APIs
- **Plan error handling** for all initialization scenarios

---

### **Component Coordination Complexity**

#### Problem Encountered
Need to coordinate multiple components (WAL, MemTable, SSTable) while maintaining consistency.

#### Root Cause
- **Multiple responsibilities**: Engine must manage WAL writes, MemTable operations, and SSTable coordination
- **State consistency**: All components must remain in sync
- **Error propagation**: Errors in one component must not corrupt others
- **Recovery complexity**: Must recover consistent state across all components

#### Solution Implemented
1. **Clear responsibility separation**: Each component handles its own domain
2. **Coordinated operations**: Engine orchestrates but doesn't duplicate logic
3. **Error handling**: Comprehensive error types with proper propagation
4. **Recovery mechanism**: Use existing WAL recovery for consistency

#### Key Learnings
1. **Orchestration vs. implementation**: Engine coordinates, components implement
2. **Error boundaries**: Clear error handling at component boundaries
3. **Recovery strategy**: Leverage existing recovery mechanisms when possible
4. **State management**: Centralized state tracking for consistency

#### Prevention
- **Design component boundaries** clearly before implementation
- **Plan error handling** across component interactions
- **Consider recovery scenarios** during initial design
- **Document coordination patterns** for future reference

---

### **WAL Rotation Strategy**

#### Problem Encountered
Need to rotate WAL files after MemTable flushes to prevent single large WAL files.

#### Root Cause
- **File size growth**: Single WAL file grows indefinitely
- **Recovery complexity**: Large WAL files slow down recovery
- **Storage efficiency**: Multiple smaller files are easier to manage

#### Solution Implemented
```rust
fn rotate_wal(&mut self) -> EngineResult<()> {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();
    
    let new_wal_path = self.config.data_dir.join(format!("wal_{}.log", timestamp));
    let new_wal = WAL::new(&new_wal_path)?;
    self.wal = new_wal;
    Ok(())
}
```

#### Key Learnings
1. **WAL rotation timing**: Rotate after successful MemTable flush
2. **File naming**: Use timestamps for unique identification
3. **Recovery order**: Must replay WAL files in chronological order
4. **Resource management**: Proper cleanup of old WAL files

#### Prevention
- **Plan file management** strategy before implementation
- **Consider recovery implications** of file organization
- **Document file naming conventions** for consistency
- **Plan cleanup strategies** for old files

---

### **Sequence Number Management**

#### Problem Encountered
Need to maintain sequence numbers across all operations and component restarts.

#### Root Cause
- **Component isolation**: Each component has its own sequence number
- **Recovery continuity**: Sequence numbers must continue after restart
- **Consistency**: All operations must have unique, ordered sequence numbers

#### Solution Implemented
```rust
// Centralized sequence number management
let sequence_number = {
    let mut seq = self.sequence_number.write().unwrap();
    *seq += 1;
    *seq
};

// Recovery updates sequence number from WAL
{
    let mut seq = self.sequence_number.write().unwrap();
    *seq = self.wal.sequence_number();
}
```

#### Key Learnings
1. **Centralized management**: Single source of truth for sequence numbers
2. **Recovery synchronization**: Update sequence numbers from recovered state
3. **Thread safety**: Use RwLock for shared sequence number access
4. **Continuity**: Sequence numbers must be monotonically increasing

#### Prevention
- **Design sequence number strategy** before implementation
- **Plan recovery synchronization** for all stateful components
- **Consider thread safety** for shared state
- **Document sequence number guarantees** for users

---

## Performance Considerations

### **MemTable Specific**
- **Size limits**: Configurable maximum size (default: 64MB)
- **Memory overhead**: ~16 bytes per entry (timestamp + sequence)
- **Insertion cost**: O(n) due to vector shifting
- **Lookup cost**: O(log n) with binary search
- **Thread safety overhead**: Minimal with RwLock

### **When to Optimize**
- **Don't optimize** until you have performance data
- **Profile first** to identify bottlenecks
- **Measure** before and after optimizations
- **Consider trade-offs** between complexity and performance

---

## Future Improvements

### **Potential Optimizations**
1. **Skip list implementation** with proper thread safety
2. **Memory pooling** for Entry allocations
3. **Batch operations** for multiple puts/deletes
4. **Compression** for large values
5. **Metrics collection** for performance monitoring

### **WAL-Specific Optimizations**
1. **Batch WAL writes**: Group multiple operations into single WAL record
2. **Compression**: Compress WAL records for large values
3. **Checksums**: Add CRC32 validation for enhanced corruption detection
4. **Parallel recovery**: Recover multiple WAL files concurrently
5. **WAL rotation**: Implement log rotation to prevent single large files

### **Performance and Memory Considerations**

#### **Buffer Size Choices**
- **Corruption recovery buffer**: 1024 bytes provides good balance between memory usage and seeking efficiency
- **WAL buffering**: `BufWriter` automatically handles optimal buffer sizes
- **Memory allocation**: Pre-allocate vectors for key/value data to avoid repeated allocations

#### **Size Limits and Validation**
```rust
// Reasonable limits prevent abuse and improve recovery
if key_len > 1024 * 1024 || value_len > 100 * 1024 * 1024 {
    return Err(WALError::InvalidRecord(format!(
        "Invalid record size: key_len={}, value_len={}", key_len, value_len
    )));
}
```
- **Key limit**: 1MB prevents extremely long keys that could cause issues
- **Value limit**: 100MB balances flexibility with memory safety
- **Recovery efficiency**: Smaller limits make corruption recovery faster

#### **Memory Allocation Patterns**
- **Header parsing**: Fixed-size arrays for headers avoid dynamic allocation
- **Data reading**: Pre-allocate vectors based on header information
- **Error handling**: Minimize allocations in error paths

### **Testing Infrastructure Insights**

#### **tempfile Crate Usage**
- **Isolated testing**: Each test gets its own temporary directory
- **Automatic cleanup**: Temporary files are automatically removed
- **Cross-platform**: Works consistently across different operating systems
- **Performance**: Faster than creating/deleting files in test directories

#### **Corruption Simulation Techniques**
```rust
// Inject corruption by appending garbage data
let mut file = OpenOptions::new().append(true).open(&wal_path).unwrap();
file.write_all(b"corrupted data here").unwrap();
```
- **Realistic testing**: Simulates actual corruption scenarios
- **Recovery validation**: Ensures recovery mechanisms work correctly
- **Edge case coverage**: Tests boundary conditions and error handling

#### **Recovery Validation Strategies**
- **Data integrity**: Verify all valid records are recovered
- **Corruption handling**: Ensure recovery continues after corruption
- **Sequence continuity**: Validate sequence numbers are maintained
- **Performance testing**: Measure recovery time for large WAL files

### **Implementation Notes**
- **Skip list**: Would require careful design to avoid borrowing issues
- **Memory pooling**: Could reduce allocation overhead
- **Batch operations**: Would improve throughput for bulk operations
- **Compression**: Consider LZ4 or Zstd for large values
- **Metrics**: Use tracing crate for structured logging

---

**Last Updated**: 2025-01-08  
**Version**: 3.0  
**Maintainer**: Development Team  

---

*This document should be updated with each major feature implementation to preserve learnings across development sessions.*
