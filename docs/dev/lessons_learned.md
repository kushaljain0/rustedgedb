# Lessons Learned - RustEdgeDB Development

## Table of Contents
1. [MemTable Implementation](#memtable-implementation)
2. [Common Rust Patterns](#common-rust-patterns)
3. [Thread Safety Patterns](#thread-safety-patterns)
4. [Testing Best Practices](#testing-best-practices)
5. [Documentation Workflow](#documentation-workflow)

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

### **Implementation Notes**
- **Skip list**: Would require careful design to avoid borrowing issues
- **Memory pooling**: Could reduce allocation overhead
- **Batch operations**: Would improve throughput for bulk operations
- **Compression**: Consider LZ4 or Zstd for large values
- **Metrics**: Use tracing crate for structured logging

---

**Last Updated**: 2025-01-08  
**Version**: 1.0  
**Maintainer**: Development Team  

---

*This document should be updated with each major feature implementation to preserve learnings across development sessions.*
