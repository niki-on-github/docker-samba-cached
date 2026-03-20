# Agent Instructions for cache-manager

This repository contains a Rust-based media RAM cache manager that uses inotify to monitor file access and vmtouch to preload video files into memory.

## Project Overview

- **Language**: Rust 2021 edition
- **Platform**: Linux-only (inotify API)
- **Binary**: Static musl build for Alpine Linux/Docker

## Architecture Notes

### IMPORTANT: Fanotify Does NOT Work in Containers
Fanotify fails with `EINVAL` in Docker/Kubernetes containers due to overlay filesystem limitations.
**DO NOT attempt to use fanotify** - use inotify only.

### Implementation Details
- Uses `inotifywait -e close_read` to detect completed video reads
- `close_read` fires AFTER file is closed, so **do not check `is_file_still_open()`**
- vmtouch is called immediately on `close_read` event
- No cooldown mechanism - kernel page cache handles duplicates
- No byte tracking - any video file read is cached (kernel handles partial reads)

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
nix develop -c cargo run
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
cargo run            # Run with default settings
```

## Code Style Guidelines

### Formatting
- Use `cargo fmt` before committing
- 4-space indentation (standard Rust)
- No trailing whitespace
- Max line length: 100 characters (soft)

### Imports
- Group imports by crate: std → external → local
- Example:
```rust
use std::collections::HashMap;
use std::path::{Path, PathBuf};
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
| parking_lot | 0.12 | Fast mutex implementation |
| tracing | 0.1 | Structured logging |
| tracing-subscriber | 0.3 | Log output formatting | |

### Configuration

The application uses environment variables for configuration:

| Variable | Description | Required |
|----------|-------------|----------|
| `CACHE_WORK_DIR` | Directory to monitor for video files | Yes |

### Logging Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `RUST_LOG` | `info` | Log level: `error`, `warn`, `info`, `debug`, `trace` |

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
- CAP_SYS_ADMIN capability (required for inotify)
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
- `player` - Auto-plays videos to generate inotify events
- `samba` - SMB server for network access to media

## Common Issues

### Fanotify Not Available in Containers
- Fanotify fails with `EINVAL` on overlay filesystems (Docker/Kubernetes)
- Use inotify only for container deployments
- Fanotify may work on bare-metal or VMs with native filesystems

### Inotify Not Available
- Ensure running as root or with CAP_SYS_ADMIN
- Check `/proc/sys/fs/inotify/max_user_watches` limits

### File Not Found After Event
- Files may be deleted between event and resolution
- Always check `path.exists()` before processing

### Event Type Logic
- Use `open` event if you need to check if file is still open
- Use `close_read` event if you want to cache after reading is complete
- **Do NOT** check `is_file_still_open()` with `close_read` - it will always be false
