# RustEdgeDB Development Playbook

## Table of Contents
1. [Versioning & Branching Rules](#versioning--branching-rules)
2. [Commits & PR Guidelines](#commits--pr-guidelines)
3. [Documentation Update Rules](#documentation-update-rules)
4. [Coding Principles](#coding-principles)
5. [Spec-Driven Development Workflow](#spec-driven-development-workflow)
6. [Release Governance](#release-governance)
7. [Learning from Mistakes](#learning-from-mistakes)

---

## Versioning & Branching Rules

### Semantic Versioning (SemVer 2.0.0)
- **MAJOR.MINOR.PATCH** format (e.g., 1.2.3)
- **MAJOR**: Breaking changes to public APIs, database schema changes, incompatible protocol changes
- **MINOR**: New features, backward-compatible additions, performance improvements
- **PATCH**: Bug fixes, security patches, documentation updates, backward-compatible

### Branch Strategy

#### Protected Branches
- **`main`**: Production-ready code, tagged releases only
- **`develop`**: Integration branch for features, nightly builds
- **`release/*`**: Release preparation branches (e.g., `release/1.2.0`)

#### Feature Development
- **`feature/*`**: Individual feature branches (e.g., `feature/query-optimizer`)
- **`bugfix/*`**: Bug fix branches (e.g., `bugfix/memory-leak`)
- **`hotfix/*`**: Critical production fixes (e.g., `hotfix/security-patch`)

#### Branch Naming Conventions
```
feature/component-description
bugfix/issue-description
hotfix/critical-fix-description
release/version-number
```

#### Branch Lifecycle
1. **Creation**: Branch from `develop` for features, from `main` for hotfixes
2. **Development**: Commit atomic changes, rebase regularly on `develop`
3. **Integration**: Create PR to `develop`, ensure CI passes
4. **Cleanup**: Delete feature branches after successful merge

---

## Commits & PR Guidelines

### Conventional Commits
Follow [Conventional Commits 1.0.0](https://www.conventionalcommits.org/) specification:

```
<type>[optional scope]: <description>

[optional body]

[optional footer(s)]
```

#### Commit Types
- **`feat`**: New feature (triggers MINOR version bump)
- **`fix`**: Bug fix (triggers PATCH version bump)
- **`docs`**: Documentation changes only
- **`style`**: Code style changes (formatting, missing semicolons, etc.)
- **`refactor`**: Code refactoring (no functional changes)
- **`perf`**: Performance improvements
- **`test`**: Adding or updating tests
- **`chore`**: Maintenance tasks, dependency updates
- **`ci`**: CI/CD configuration changes
- **`build`**: Build system or external dependency changes
- **`revert`**: Revert previous commit

#### Commit Examples
```
feat(database): implement B-tree index for range queries
fix(parser): resolve SQL injection vulnerability in prepared statements
docs(api): update client library usage examples
refactor(storage): extract common storage traits
test(query): add integration tests for complex joins
chore(deps): update tokio to 1.35.0
```

### Atomic Commits
- **One logical change per commit**
- **Self-contained**: Each commit should compile and pass tests
- **Descriptive**: Clear what and why, not how
- **Small**: Aim for commits under 50 lines of code changes

### Pull Request Guidelines

#### PR Title Format
```
<type>(<scope>): <description>
```

#### PR Description Template
```markdown
## Summary
Brief description of changes

## Type of Change
- [ ] Bug fix (non-breaking change)
- [ ] New feature (non-breaking change)
- [ ] Breaking change (fix or feature that causes existing functionality to not work as expected)
- [ ] Documentation update

## Testing
- [ ] Unit tests pass
- [ ] Integration tests pass
- [ ] Performance benchmarks within acceptable range
- [ ] Security scan completed

## Checklist
- [ ] Code follows style guidelines
- [ ] Self-review completed
- [ ] Documentation updated
- [ ] Breaking changes documented
- [ ] Tests added for new functionality
```

#### PR Review Requirements
- **Minimum 2 approvals** from maintainers
- **All CI checks must pass** (build, test, format, clippy)
- **No merge conflicts**
- **Up-to-date with target branch**

---

## Documentation Update Rules

### Documentation Hierarchy
1. **`docs/spec.md`**: Core specification (versioned, breaking changes require major version)
2. **`docs/dev/*`**: Developer documentation (internal processes)
3. **`docs/user/*`**: User-facing documentation (API references, guides)

### Documentation Standards
- **Markdown format** with consistent structure
- **Code examples** for all public APIs
- **Version compatibility** clearly marked
- **Breaking changes** prominently documented
- **Regular reviews** every 3 months

### Update Triggers
- **New features**: Update user docs and examples
- **API changes**: Update spec.md with version compatibility
- **Bug fixes**: Update relevant troubleshooting sections
- **Process changes**: Update dev documentation

### Documentation Review Process
1. **Technical accuracy** review by domain experts
2. **User experience** review by technical writers
3. **Accessibility** review for inclusive language
4. **Version compatibility** verification

---

## Coding Principles

### Safe Rust Practices
- **Memory safety**: Leverage Rust's ownership system
- **No unsafe blocks** without thorough justification and documentation
- **Error handling**: Use `Result<T, E>` and `Option<T>` appropriately
- **Panic avoidance**: Handle errors gracefully, avoid `unwrap()` in production code

### Trait Design
- **Interface segregation**: Keep traits focused and cohesive
- **Default implementations**: Provide sensible defaults where possible
- **Generic constraints**: Use trait bounds for compile-time guarantees
- **Trait objects**: Prefer generics over trait objects for performance

### Error Handling Strategy
```rust
// Use custom error types with thiserror
#[derive(Debug, thiserror::Error)]
pub enum DatabaseError {
    #[error("Connection failed: {0}")]
    ConnectionFailed(String),
    #[error("Query execution failed: {0}")]
    QueryFailed(#[from] QueryError),
    #[error("Transaction conflict: {0}")]
    TransactionConflict(String),
}

// Return Results, not Options for recoverable errors
pub fn execute_query(query: &str) -> Result<QueryResult, DatabaseError> {
    // Implementation
}
```

### Testing Strategy

#### Test Pyramid
- **Unit tests**: 70% - Fast, isolated, test individual functions
- **Integration tests**: 20% - Test component interactions
- **End-to-end tests**: 10% - Test complete workflows

#### Test Requirements
- **Coverage target**: Minimum 80% line coverage
- **Performance tests**: Benchmark critical paths
- **Property-based tests**: Use `proptest` for data structure validation
- **Fuzz testing**: Use `cargo fuzz` for input validation

#### Test Organization
```rust
#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_function_name() {
        // Arrange
        let input = "test";
        
        // Act
        let result = function(input);
        
        // Assert
        assert_eq!(result, expected);
    }
    
    #[test]
    #[should_panic(expected = "specific error message")]
    fn test_panic_condition() {
        // Test that panics with specific message
    }
}
```

---

## Spec-Driven Development Workflow

### Specification First
1. **Update `docs/spec.md`** before implementing features
2. **Version compatibility** clearly marked
3. **Breaking changes** require major version bump
4. **Reference implementation** follows spec exactly

### Development Cycle
1. **Spec Review**: Team reviews specification changes
2. **Implementation**: Code follows approved specification
3. **Validation**: Tests verify spec compliance
4. **Documentation**: Update user docs with examples

### Spec Compliance Testing
- **Unit tests** verify individual spec requirements
- **Integration tests** verify component interactions
- **Compliance tests** verify full specification adherence
- **Regression tests** ensure spec changes don't break existing functionality

---

## Release Governance

### Release Schedule
- **Patch releases**: As needed for critical fixes
- **Minor releases**: Every 6 weeks for new features
- **Major releases**: Every 6 months for breaking changes

### Release Process
1. **Feature freeze**: 2 weeks before release
2. **Release branch**: Create `release/X.Y.Z` from `develop`
3. **Testing**: Full test suite, performance benchmarks
4. **Documentation**: Update release notes, migration guides
5. **Tagging**: Create annotated git tag
6. **Deployment**: Merge to `main`, deploy to production

### Release Criteria
- **All tests pass** (unit, integration, performance)
- **Security scan clean** (no high/critical vulnerabilities)
- **Performance benchmarks** within acceptable range
- **Documentation complete** and accurate
- **Breaking changes** documented with migration path

### Release Notes Format
```markdown
# RustEdgeDB vX.Y.Z

## Breaking Changes
- List breaking changes with migration instructions

## New Features
- List new features with usage examples

## Improvements
- List performance improvements and optimizations

## Bug Fixes
- List bug fixes with issue references

## Security
- List security-related changes

## Migration Guide
- Step-by-step migration instructions for breaking changes
```

### Post-Release Activities
1. **Monitor metrics** for 48 hours
2. **Collect feedback** from users
3. **Document lessons learned**
4. **Plan next release cycle**

---

## Enforcement & Compliance

### Code Review Checklist
- [ ] Follows coding principles
- [ ] Includes appropriate tests
- [ ] Documentation updated
- [ ] No breaking changes without major version
- [ ] Performance impact assessed
- [ ] Security implications reviewed

### Continuous Integration
- **Automated checks** for all requirements
- **Blocking merges** until compliance verified
- **Regular audits** of development practices
- **Performance regression** detection

### Quality Gates
- **Test coverage** minimum 80%
- **Performance benchmarks** within 5% of baseline
- **Security scan** no high/critical issues
- **Documentation coverage** 100% for public APIs

This playbook ensures consistent, high-quality development practices across the RustEdgeDB project.

---

## Learning from Mistakes

### Documenting Mistakes
- **`docs/lessons-learned.md`**: Comprehensive record of bugs, issues, and their solutions.
- **Include**:
  - **Bug description**
  - **Root cause analysis**
  - **Solution steps**
  - **Impact on users**
  - **Preventive measures**

### Analyzing Mistakes
- **Identify patterns** in recurring issues
- **Understand why** they occurred
- **Document** findings for future reference

### Preventing Mistakes
- **Code reviews** for potential issues
- **Testing** for edge cases
- **Documentation** for common pitfalls
