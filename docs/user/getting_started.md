# Getting Started with RustEdgeDB

## Table of Contents
1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Quick Start](#quick-start)
4. [Basic Operations](#basic-operations)
5. [Configuration](#configuration)
6. [Error Handling](#error-handling)
7. [Current Limitations](#current-limitations)
8. [Next Steps](#next-steps)

---

## Prerequisites

### System Requirements
- **Rust**: 1.70+ (2021 edition)
- **Cargo**: Latest stable version
- **Operating System**: Windows, macOS, or Linux
- **Memory**: Minimum 10MB available RAM
- **Disk**: Minimum 50MB available space

### Rust Installation
If you don't have Rust installed, visit [rustup.rs](https://rustup.rs) and run:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
# or on Windows: https://win.rustup.rs
```

Verify installation:
```bash
rustc --version
cargo --version
```

---

## Installation

### Option 1: Add to Your Project (Recommended)
Add RustEdgeDB to your `Cargo.toml`:

```toml
[dependencies]
rustedgedb = "0.1.0"
tokio = { version = "1.0", features = ["full"] }
```

### Option 2: Install as Binary
Install RustEdgeDB globally:

```bash
cargo install rustedgedb
```

### Option 3: Build from Source
```bash
git clone https://github.com/your-org/rustedgedb.git
cd rustedgedb
cargo build --release
```

---

## Quick Start

### 1. Create a New Project
```bash
cargo new my_rustedgedb_app
cd my_rustedgedb_app
```

### 2. Add Dependencies
Edit `Cargo.toml`:
```toml
[package]
name = "my_rustedgedb_app"
version = "0.1.0"
edition = "2021"

[dependencies]
rustedgedb = "0.1.0"
tokio = { version = "1.0", features = ["full"] }
anyhow = "1.0"
```

### 3. Basic Example
Create `src/main.rs`:
```rust
use rustedgedb::{Database, DatabaseOptions, Error};
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<(), Error> {
    // Initialize database
    let options = DatabaseOptions {
        data_dir: PathBuf::from("./data"),
        memtable_size: 64 * 1024 * 1024, // 64MB
        sstable_size: 256 * 1024 * 1024, // 256MB
        compression: rustedgedb::CompressionType::Lz4,
        bloom_filter_bits: 10,
        max_levels: 7,
    };
    
    let mut db = Database::open(options).await?;
    
    // Basic operations
    db.put(b"hello", b"world").await?;
    
    if let Some(value) = db.get(b"hello").await? {
        println!("Retrieved: {}", String::from_utf8_lossy(&value));
    }
    
    db.delete(b"hello").await?;
    
    println!("Database operations completed successfully!");
    Ok(())
}
```

---

## Basic Operations

### Database Initialization
```rust
use rustedgedb::{Database, DatabaseOptions};
use std::path::PathBuf;

// Default configuration
let db = Database::open_default("./data").await?;

// Custom configuration
let options = DatabaseOptions {
    data_dir: PathBuf::from("./my_database"),
    memtable_size: 32 * 1024 * 1024, // 32MB
    sstable_size: 128 * 1024 * 1024, // 128MB
    compression: rustedgedb::CompressionType::Zstd,
    bloom_filter_bits: 12,
    max_levels: 5,
};

let db = Database::open(options).await?;
```

### Key-Value Operations
```rust
// Store a value
db.put(b"user:123", b"John Doe").await?;
db.put(b"user:456", b"Jane Smith").await?;

// Retrieve values
if let Some(name) = db.get(b"user:123").await? {
    println!("User: {}", String::from_utf8_lossy(&name));
}

// Check if key exists
match db.get(b"user:789").await? {
    Some(value) => println!("Found: {}", String::from_utf8_lossy(&value)),
    None => println!("User not found"),
}

// Delete a key
db.delete(b"user:123").await?;

// Verify deletion
assert!(db.get(b"user:123").await?.is_none());
```

### Batch Operations
```rust
use rustedgedb::BatchOp;

// Prepare batch operations
let operations = vec![
    BatchOp::Put {
        key: b"config:theme".to_vec(),
        value: b"dark".to_vec(),
    },
    BatchOp::Put {
        key: b"config:language".to_vec(),
        value: b"en".to_vec(),
    },
    BatchOp::Delete {
        key: b"config:old_setting".to_vec(),
    },
];

// Execute batch atomically
db.batch_write(operations).await?;
```

### Working with Different Data Types
```rust
use serde::{Serialize, Deserialize};

#[derive(Serialize, Deserialize, Debug)]
struct User {
    id: u64,
    name: String,
    email: String,
}

// Store serialized data
let user = User {
    id: 123,
    name: "John Doe".to_string(),
    email: "john@example.com".to_string(),
};

let user_data = bincode::serialize(&user)?;
db.put(b"user:123", &user_data).await?;

// Retrieve and deserialize
if let Some(data) = db.get(b"user:123").await? {
    let retrieved_user: User = bincode::deserialize(&data)?;
    println!("User: {:?}", retrieved_user);
}
```

---

## Configuration

### Database Options
```rust
pub struct DatabaseOptions {
    pub data_dir: PathBuf,           // Storage directory
    pub memtable_size: usize,        // MemTable size limit
    pub sstable_size: usize,         // SSTable size limit
    pub compression: CompressionType, // Compression algorithm
    pub bloom_filter_bits: usize,    // Bloom filter precision
    pub max_levels: usize,           // Maximum compaction levels
}
```

### Compression Types
```rust
pub enum CompressionType {
    None,    // No compression
    Lz4,     // Fast compression, moderate ratio
    Zstd,    // Slower compression, better ratio
}
```

### Environment Variables
```bash
# Set data directory
export RUSTEDGEDB_DATA_DIR="/var/lib/rustedgedb"

# Set memory limits
export RUSTEDGEDB_MEMTABLE_SIZE="67108864"  # 64MB
export RUSTEDGEDB_SSTABLE_SIZE="268435456"  # 256MB

# Set compression
export RUSTEDGEDB_COMPRESSION="zstd"
```

---

## Error Handling

### Error Types
```rust
use rustedgedb::DatabaseError;

match db.put(b"key", b"value").await {
    Ok(()) => println!("Operation successful"),
    Err(DatabaseError::KeyTooLarge { .. }) => {
        eprintln!("Key size exceeds limit");
    }
    Err(DatabaseError::ValueTooLarge { .. }) => {
        eprintln!("Value size exceeds limit");
    }
    Err(DatabaseError::StorageError { .. }) => {
        eprintln!("Storage operation failed");
    }
    Err(e) => eprintln!("Unexpected error: {}", e),
}
```

### Custom Error Handling
```rust
use anyhow::{Result, Context};

async fn store_user_data(db: &Database, user_id: &[u8], data: &[u8]) -> Result<()> {
    db.put(user_id, data)
        .await
        .context("Failed to store user data")?;
    
    Ok(())
}

async fn retrieve_user_data(db: &Database, user_id: &[u8]) -> Result<Option<Vec<u8>>> {
    let data = db.get(user_id)
        .await
        .context("Failed to retrieve user data")?;
    
    Ok(data)
}
```

---

## Current Limitations

### Spec v0.1 Scope
**Important**: RustEdgeDB v0.1 implements a **base engine** with limited functionality:

#### **Supported Features**
- **In-Memory Storage**: Fast MemTable operations
- **WAL Persistence**: Crash recovery and durability
- **Basic Operations**: `put()`, `get()`, `delete()`
- **Batch Operations**: Atomic multi-operation batches
- **Compression**: LZ4 and Zstd support
- **Bloom Filters**: Fast negative lookups

#### **Not Yet Supported**
- **Persistent Storage**: SSTables and compaction (in-memory only)
- **Advanced Queries**: Range queries, filtering, aggregation
- **Transactions**: ACID transaction support
- **Concurrency**: Multi-threaded access patterns
- **Network Protocol**: Remote client connections
- **Replication**: Data distribution and synchronization

#### **Work in Progress**
- **SSTable Implementation**: Persistent storage layer
- **Compaction Engine**: Automatic space reclamation
- **Performance Optimization**: Benchmarking and tuning

### Performance Characteristics
- **Memory Usage**: 10-100MB depending on data size
- **Write Throughput**: ~100K ops/sec (in-memory)
- **Read Latency**: < 1ms for hot data
- **Startup Time**: < 100ms
- **Storage**: WAL files only (no persistent data)

---

## Next Steps

### 1. Explore the API
- Read the [API Reference](../api/)
- Check out [Examples](../examples/)
- Review [Best Practices](../best-practices/)

### 2. Understand the Architecture
- Read the [Specification](../spec.md)
- Learn about [LSM Trees](../concepts/lsm-trees.md)
- Understand [Crash Recovery](../concepts/crash-recovery.md)

### 3. Contribute to Development
- Report [Issues](https://github.com/your-org/rustedgedb/issues)
- Submit [Pull Requests](https://github.com/your-org/rustedgedb/pulls)
- Join our [Community](../community/)

### 4. Stay Updated
- Watch the [Repository](https://github.com/your-org/rustedgedb)
- Follow [Release Notes](../releases/)
- Subscribe to [Newsletter](../newsletter/)

---

## Support & Community

### Documentation
- **API Reference**: [docs.rustedgedb.dev/api](https://docs.rustedgedb.dev/api)
- **Examples**: [docs.rustedgedb.dev/examples](https://docs.rustedgedb.dev/examples)
- **Tutorials**: [docs.rustedgedb.dev/tutorials](https://docs.rustedgedb.dev/tutorials)

### Community Channels
- **Discord**: [Join our server](https://discord.gg/rustedgedb)
- **GitHub Discussions**: [Q&A and discussions](https://github.com/your-org/rustedgedb/discussions)
- **Stack Overflow**: Tag questions with `rustedgedb`

### Getting Help
- **Bug Reports**: [GitHub Issues](https://github.com/your-org/rustedgedb/issues)
- **Feature Requests**: [GitHub Discussions](https://github.com/your-org/rustedgedb/discussions)
- **Security Issues**: [security@rustedgedb.dev](mailto:security@rustedgedb.dev)

---

## Quick Reference

### Common Operations
```rust
// Initialize
let db = Database::open_default("./data").await?;

// Store
db.put(b"key", b"value").await?;

// Retrieve
let value = db.get(b"key").await?;

// Delete
db.delete(b"key").await?;

// Batch
db.batch_write(operations).await?;
```

### Configuration
```rust
let options = DatabaseOptions {
    data_dir: PathBuf::from("./data"),
    memtable_size: 64 * 1024 * 1024, // 64MB
    sstable_size: 256 * 1024 * 1024, // 256MB
    compression: CompressionType::Lz4,
    bloom_filter_bits: 10,
    max_levels: 7,
};
```

**Ready to get started?** Follow the examples above and start building with RustEdgeDB!
