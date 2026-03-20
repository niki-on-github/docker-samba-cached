# Agent Instructions for cache-manager

This repository contains a Rust-based media RAM cache manager that uses fanotify to monitor file access and vmtouch to preload video files into memory.

## Project Overview

- **Language**: Rust 2021 edition
- **Platform**: Linux-only (fanotify API)
- **Binary**: Static musl build for Alpine Linux/Docker

## Build Commands

### Local Development (Nix)

```bash
# Enter dev shell
nix develop

# Or using nix-portable
nix-portable nix develop

# Run commands in shell
nix develop -c cargo check
nix develop -c cargo build
nix develop -c cargo build --release
nix develop -c cargo test
RUST_LOG=info nix develop -c cargo run
```

### Docker Build

```bash
docker build -t cache-manager .
```

### Rust Commands (Direct Install)

```bash
cargo check          # Type-check without building
cargo build          # Debug build
cargo build --release # Optimized release build
cargo test           # Run tests
cargo run -- <args>  # Run with arguments
```

## Code Style Guidelines

### Formatting
- Use `cargo fmt` before committing
- 4-space indentation (standard Rust)
- No trailing whitespace
- Max line length: 100 characters (soft)

### Imports
- Group imports by crate: std → external → local
- Use absolute paths for clarity in fanotify bindings
- Example:
```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use nix::sys::fanotify::{Fanotify, InitFlags, MarkFlags, MaskFlags};
use parking_lot::Mutex;
use tracing::{debug, error, info, warn};
```

### Types
- Use explicit types for public APIs (function signatures)
- Prefer `&Path` over `&PathBuf` for function parameters
- Use `PathBuf` when ownership is needed
- Use `parking_lot::Mutex` instead of `std::sync::Mutex` (faster)
- Use `Option<T>` for nullable values, not `null`

### Naming Conventions
- **Variables/Functions**: `snake_case`
- **Types/Structs/Enums**: `PascalCase`
- **Constants**: `SCREAMING_SNAKE_CASE`
- **Private fields**: `_leading_underscore` for unused

### Error Handling
- Use `?` operator for propagating errors
- Use `match` for exhaustive error handling
- Use `if let Err(_) =` for non-fatal errors (logging + continuing)
- Never silently ignore errors unless intentionally skipping

```rust
// Good: Log and continue
if let Err(e) = some_operation() {
    warn!("Operation failed: {}", e);
    return;
}

// Good: Early return on fatal error
let value = some_operation().map_err(|e| format!("Failed: {}", e))?;
```

### Logging
- Use `tracing` crate for structured logging
- Log levels: `error!` (fatal), `warn!` (recoverable), `info!` (important), `debug!` (verbose)
- Always include context in log messages

## Project Structure

```
/workspace/
├── src/main.rs              # Main application
├── Cargo.toml               # Dependencies and metadata
├── Cargo.lock               # Locked dependency versions
├── flake.nix                # Nix dev shell
├── flake.lock               # Nix lock file
├── Dockerfile               # Multi-stage Docker build
├── docker-compose.yml       # Docker Compose for testing
├── test-media/              # Test video files (gitignored)
│   └── .gitignore
└── rootfs/                  # Docker init scripts
    └── etc/services.d/cache-manager/run
```

## Dependencies

| Crate | Version | Purpose |
|-------|---------|---------|
| nix | 0.31 (fanotify feature) | Linux fanotify API bindings |
| parking_lot | 0.12 | Fast mutex implementation |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 | Log output formatting |

### Configuration

The application uses environment variables for configuration:

| Variable | Description | Required |
|----------|-------------|----------|
| `CACHE_WORK_DIR` | Directory to monitor for video files | Yes |

## Testing

This project currently has no unit tests. When adding tests:

```bash
# Run all tests
cargo test

# Run specific test
cargo test test_name

# Run with output
cargo test -- --nocapture
```

## Adding Dependencies

When adding new dependencies:

1. Add to `Cargo.toml` with version constraints
2. Run `cargo check` to verify compilation
3. Update this AGENTS.md if adding important dependencies

## Docker Environment

The binary runs in Docker with:
- Alpine Linux base
- CAP_SYS_ADMIN capability (required for fanotify)
- Mount propagation for watching paths

### Docker Compose Testing

```bash
# Add test video files to test-media/ directory

# Start all services
docker compose up -d

# View cache-manager logs
docker compose logs -f cache-manager

# View all logs
docker compose logs -f

# Stop all services
docker compose down

# Rebuild and start
docker compose up -d --build
```

**Services:**
- `cache-manager` - The main cache manager (monitors /media)
- `player` - Auto-plays videos to generate fanotify events
- `samba` - SMB server for network access to media

## Common Issues

### Fanotify Initialization Fails
- Ensure running as root or with CAP_SYS_ADMIN
- Check `/proc/sys/fs/fanotify/max_user_marks` limits

### File Not Found After Fanotify Event
- Files may be deleted between event and resolution
- Always check `path.exists()` before processing
