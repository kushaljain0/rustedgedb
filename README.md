# RustEdgeDB

RustEdgeDB — a deterministic, embeddable, edge-first database engine.

## Project Structure

- `src/` - Main source code
- `docs/` - Documentation
  - `spec.md` - Specification document with versioned sections
  - `dev/` - Developer documentation
    - `coding_principles.md` - Coding principles and guidelines
    - `process.md` - Development process and workflow
  - `user/` - User documentation
    - `getting_started.md` - Getting started guide
- `tests/` - Integration tests

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

