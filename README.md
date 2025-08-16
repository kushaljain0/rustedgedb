# RustEdgeDB

RustEdgeDB — a deterministic, embeddable, edge-first database engine.

## Project Structure

- `src/` - Main source code
  - `memtable.rs` - In-memory table implementation with sorted vector storage
  - `wal.rs` - Write-Ahead Log for durability and crash recovery
- `docs/` - Documentation
  - `spec.md` - Specification document with versioned sections
  - `dev/` - Developer documentation
    - `coding_principles.md` - Coding principles and guidelines
    - `process.md` - Development process and workflow
    - `lessons_learned.md` - Lessons learned from implementation challenges
  - `user/` - User documentation
    - `getting_started.md` - Getting started guide
- `tests/` - Integration tests

## Current Implementation Status

### ✅ Implemented Components
- **MemTable**: In-memory table with sorted vector storage, O(log n) operations
- **WAL**: Write-Ahead Log with append-only file, corruption recovery, and MemTable replay
- **Core Infrastructure**: Error handling, logging, testing framework

### 🚧 In Progress
- **SSTable**: Immutable, persistent storage for flushed data
- **Compaction Engine**: Leveled compaction strategy

### 📋 Planned
- **Database Engine**: Main coordination layer
- **API Layer**: Public interface for database operations
- **Performance Optimizations**: Bloom filters, compression

## Development

This project uses Rust 2024 edition. To get started:

```bash
# Build the project
cargo build

# Run tests
cargo test

# Format code
cargo fmt

# Run clippy
cargo clippy
```

## CI/CD

The project uses GitHub Actions for continuous integration, running:
- `cargo fmt` - Code formatting
- `cargo clippy` - Linting
- `cargo test` - Test suite

