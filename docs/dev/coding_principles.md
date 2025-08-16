# RustEdgeDB Coding Principles

## Table of Contents
1. [Rust Safety Practices](#rust-safety-practices)
2. [Code Style Standards](#code-style-standards)
3. [Error Handling](#error-handling)
4. [Observability](#observability)
5. [Testing Rules](#testing-rules)
6. [Property-Based Testing](#property-based-testing)
7. [Enforcement](#enforcement)

---

## Rust Safety Practices

### MANDATORY: Memory Safety
- **`#![deny(unsafe_code)]`** must be present in all crate root files
- **No `unsafe` blocks** without written justification and maintainer approval
- **All unsafe code** must be documented with safety invariants
- **Memory leaks prohibited** - use RAII patterns consistently

### MANDATORY: Panic Prevention
- **No `unwrap()`, `expect()`, `panic!()` in production code**
- **No `unreachable!()` or `todo!()` in production code**
- **Use `Result<T, E>` for all fallible operations**
- **Use `Option<T>` for nullable values**
- **Handle all error cases explicitly**

### MANDATORY: Ownership & Borrowing
- **Leverage Rust's ownership system** for memory safety
- **Prefer borrowing over cloning** when possible
- **Use `Cow<T>` for avoiding unnecessary allocations**
- **Implement `Clone` only when absolutely necessary**
- **Use `Arc<T>` for shared ownership, `Rc<T>` for single-threaded**

### MANDATORY: Thread Safety
- **All public APIs must be thread-safe** unless explicitly documented as single-threaded
- **Use `Send + Sync` bounds** for types crossing thread boundaries
- **Prefer `Mutex<T>` over `RwLock<T>` for simple cases**
- **Use `parking_lot` locks** for better performance

---

## Code Style Standards

### MANDATORY: Formatting & Linting
- **`cargo fmt --all` must pass** before any commit
- **`cargo clippy --all-targets --all-features -- -D warnings` must pass**
- **No warnings allowed** in production code
- **Use `rustfmt.toml`** for consistent formatting rules

### MANDATORY: Module Size Limits
- **Maximum 500 lines of code per module**
- **Maximum 100 lines per function**
- **Maximum 20 lines per method**
- **Split large modules** into smaller, focused modules
- **Extract complex logic** into separate functions

### MANDATORY: Naming Conventions
- **`snake_case` for variables, functions, and modules**
- **`SCREAMING_SNAKE_CASE` for constants**
- **`PascalCase` for types, traits, and enums**
- **`UPPER_CASE` for associated constants**
- **Descriptive names** - avoid abbreviations and single letters

### MANDATORY: Documentation
- **Document all public APIs** with `///` doc comments
- **Include usage examples** in documentation
- **Document error conditions** and return values
- **Use doc tests** for code examples
- **Keep documentation synchronized** with code changes

---

## Error Handling

### MANDATORY: Error Types
- **Use `thiserror` for all custom error types**
- **Implement `std::error::Error` trait** for all errors
- **Provide meaningful error messages** with context
- **Use `#[from]` attribute** for error conversion
- **Implement `Display` trait** for user-facing error messages

### MANDATORY: Error Handling Patterns
```rust
// REQUIRED: Use thiserror for custom errors
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection failed: {reason}")]
    ConnectionFailed { reason: String },
    
    #[error("Query execution failed: {0}")]
    QueryFailed(#[from] QueryError),
    
    #[error("Transaction conflict: {operation}")]
    TransactionConflict { operation: String },
}

// REQUIRED: Return Results, not Options for fallible operations
pub fn execute_query(query: &str) -> Result<QueryResult, DatabaseError> {
    // Implementation must handle all error cases
}

// REQUIRED: Use ? operator for error propagation
pub fn process_data(data: &[u8]) -> Result<ProcessedData, DatabaseError> {
    let parsed = parse_data(data)?;
    let validated = validate_data(parsed)?;
    Ok(ProcessedData::new(validated))
}
```

### MANDATORY: Error Context
- **Include relevant context** in error messages
- **Log errors at appropriate levels** using structured logging
- **Provide actionable error messages** for users
- **Include error codes** for programmatic error handling

---

## Observability

### MANDATORY: Logging Framework
- **Use `tracing` crate** for all logging and instrumentation
- **No `println!`, `eprintln!`, or `log` crate** allowed
- **Structured logging** with key-value pairs
- **Consistent log levels** across the application

### MANDATORY: Logging Standards
```rust
// REQUIRED: Use tracing for all logging
use tracing::{info, warn, error, debug, trace};

// REQUIRED: Structured logging with context
info!(
    query = %query,
    duration_ms = %duration.as_millis(),
    "Query executed successfully"
);

// REQUIRED: Error logging with context
error!(
    error = %err,
    query = %query,
    user_id = %user_id,
    "Query execution failed"
);

// REQUIRED: Debug logging for development
debug!(
    connection_id = %conn_id,
    "Establishing database connection"
);
```

### MANDATORY: Metrics & Tracing
- **Use `tracing` spans** for request tracing
- **Implement metrics collection** for critical operations
- **Track performance metrics** (latency, throughput, error rates)
- **Use consistent metric names** and labels

### MANDATORY: Performance Monitoring
- **Instrument all public APIs** with timing
- **Track resource usage** (memory, CPU, I/O)
- **Monitor error rates** and failure patterns
- **Alert on performance degradation**

---

## Testing Rules

### MANDATORY: Test Coverage
- **Minimum 80% line coverage** required
- **100% coverage for critical paths** (authentication, data validation)
- **Test all error conditions** and edge cases
- **Test all public APIs** with various inputs

### MANDATORY: Unit Testing
```rust
// REQUIRED: Test organization
#[cfg(test)]
mod tests {
    use super::*;
    
    // REQUIRED: Test naming convention
    #[test]
    fn test_function_name_with_expected_behavior() {
        // Arrange
        let input = "test_input";
        let expected = "expected_output";
        
        // Act
        let result = function(input);
        
        // Assert
        assert_eq!(result, expected);
    }
    
    // REQUIRED: Test error conditions
    #[test]
    fn test_function_returns_error_for_invalid_input() {
        let invalid_input = "invalid";
        
        let result = function(invalid_input);
        
        assert!(result.is_err());
        match result {
            Err(DatabaseError::InvalidInput { .. }) => {},
            _ => panic!("Expected InvalidInput error"),
        }
    }
    
    // REQUIRED: Test edge cases
    #[test]
    fn test_function_handles_empty_input() {
        let result = function("");
        assert!(result.is_ok());
    }
}
```

### MANDATORY: Integration Testing
- **Test component interactions** and workflows
- **Use test databases** with isolated data
- **Test error propagation** across components
- **Test concurrent operations** and race conditions

### MANDATORY: Test Data
- **Use factories or builders** for test data creation
- **Avoid hardcoded test values** in assertions
- **Use property-based testing** for data validation
- **Clean up test data** after each test

---

## Property-Based Testing

### MANDATORY: Proptest Usage
- **Use `proptest` crate** for property-based testing
- **Test data structure properties** (invariants, round-trip)
- **Test algorithm properties** (commutativity, associativity)
- **Test serialization/deserialization** round-trips

### MANDATORY: Property Test Implementation
```rust
// REQUIRED: Property-based testing setup
use proptest::prelude::*;

proptest! {
    // REQUIRED: Test data structure invariants
    #[test]
    fn test_b_tree_invariants(keys: Vec<i32>) {
        let mut tree = BTree::new();
        
        // Insert keys
        for key in keys {
            tree.insert(key, key.to_string());
        }
        
        // REQUIRED: Verify invariants
        prop_assert!(tree.is_valid());
        prop_assert_eq!(tree.len(), keys.len());
    }
    
    // REQUIRED: Test round-trip serialization
    #[test]
    fn test_serialization_round_trip(data: Vec<u8>) {
        let original = Data::from_bytes(&data);
        let serialized = original.serialize()?;
        let deserialized = Data::deserialize(&serialized)?;
        
        prop_assert_eq!(original, deserialized);
    }
    
    // REQUIRED: Test algorithm properties
    #[test]
    fn test_query_optimization_commutative(
        queries: Vec<Query>,
        data: Vec<Data>
    ) {
        let mut db = Database::new();
        db.insert_batch(data);
        
        let result1 = db.execute_queries(&queries);
        let result2 = db.execute_queries(&queries.into_iter().rev().collect());
        
        prop_assert_eq!(result1, result2);
    }
}
```

### MANDATORY: Property Test Coverage
- **Test all data structure operations** with various inputs
- **Test serialization formats** with edge cases
- **Test algorithm correctness** with random inputs
- **Test performance properties** under load

---

## Enforcement

### MANDATORY: Pre-commit Checks
- **`cargo check` must pass**
- **`cargo fmt --all -- --check` must pass**
- **`cargo clippy --all-targets --all-features -- -D warnings` must pass**
- **`cargo test` must pass**
- **Coverage report** must meet minimum requirements

### MANDATORY: CI/CD Enforcement
- **All checks must pass** before merge
- **Coverage thresholds enforced** by CI
- **Performance regression detection** automated
- **Security scanning** required for all changes

### MANDATORY: Code Review Requirements
- **Reviewers must verify** all coding principles
- **No exceptions** to mandatory rules without maintainer approval
- **Documentation updates** required for rule violations
- **Performance impact assessment** for all changes

### MANDATORY: Violation Consequences
- **PR blocked** until violations resolved
- **Code review rejected** for principle violations
- **Documentation required** for any rule exceptions
- **Regular audits** of compliance

---

## Compliance Checklist

### Before Every Commit
- [ ] `#![deny(unsafe_code)]` present in crate root
- [ ] No `unwrap()`, `expect()`, or `panic!()` calls
- [ ] All public APIs return `Result<T, E>` for fallible operations
- [ ] `cargo fmt --all` passes
- [ ] `cargo clippy` passes with no warnings
- [ ] All tests pass
- [ ] Documentation updated for public API changes

### Before Every PR
- [ ] Code coverage meets 80% minimum
- [ ] Property-based tests implemented for new data structures
- [ ] Error handling follows `thiserror` patterns
- [ ] Logging uses `tracing` crate
- [ ] Module size under 500 lines
- [ ] Function size under 100 lines
- [ ] Performance impact assessed
- [ ] Security implications reviewed

### Before Every Release
- [ ] All mandatory rules verified
- [ ] Performance benchmarks within acceptable range
- [ ] Security scan completed
- [ ] Documentation coverage 100% for public APIs
- [ ] Error handling tested for all edge cases
- [ ] Observability metrics implemented

**These coding principles are MANDATORY and non-negotiable. Violations will block code reviews and PRs until resolved.**
