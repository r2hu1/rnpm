# rnpm - A Fast Rust-based Node Package Manager

A high-performance package manager for Node.js projects, written in Rust. Features concurrent dependency resolution and parallel downloads for significantly faster installation times.

## Features

- **Concurrent Dependency Resolution**: Resolves up to 30 packages simultaneously using parallel async workers
- **Smart Deduplication**: Each package is fetched only once, even if multiple dependencies require it
- **Real-time Progress Tracking**: Live updates showing resolved packages and pending work
- **Metadata Caching**: In-memory cache prevents redundant API calls
- **Retry Logic**: Automatic retry (3 attempts) with exponential backoff for failed requests
- **Connection Pooling**: Optimized HTTP client with keep-alive and connection reuse
- **Lockfile Support**: Uses `rnpm.lock` for deterministic installs
- **Standard Commands**: Supports `install`, `update`, `add`, `remove`, and `run`

## Installation

```bash
cargo build --release
./target/release/rnpm install
```

## Usage

### Install Dependencies
```bash
rnpm install
```
Installs all dependencies from `package.json` or `rnpm.lock`.

### Update Dependencies
```bash
rnpm update
```
Updates all dependencies to their latest versions matching the specified ranges.

### Add a Package
```bash
rnpm add <package-name>
rnpm add <package-name> -D  # Add to devDependencies
```

### Remove a Package
```bash
rnpm remove <package-name>
```

### Run Scripts
```bash
rnpm run <script-name>
```
Runs a script defined in `package.json`.

## Performance Optimizations

### 1. Parallel Resolution
- Spawns concurrent tasks for each package
- Up to 30 packages resolved simultaneously
- Dependencies are queued immediately upon resolution

### 2. Smart Caching
- In-memory metadata cache avoids duplicate API calls
- HTTP connection pooling reduces latency
- Request abbreviated npm metadata format

### 3. Concurrent Downloads
- Up to 15 packages downloaded simultaneously
- Semaphore-controlled to prevent overwhelming the system
- 120-second timeout for large packages

### 4. Progress Visibility
- Real-time updates: "Resolving X... (Y resolved, Z pending)"
- Shows checkmarks for completed packages
- Final count display

## Architecture

### Resolver (`src/resolver.rs`)
Uses a fire-and-forget task spawning approach:
1. All top-level packages added to pending queue
2. Worker tasks spawned for each pending package
3. Each task fetches metadata and queues its dependencies
4. Deduplication ensures each package processed once

### Downloader (`src/downloader.rs`)
Handles package download and extraction:
1. Downloads tarball from npm registry
2. Extracts to temporary directory
3. Moves `package/` subdirectory to `node_modules/`
4. Handles edge cases (no package dir, multiple files)

### Registry Client (`src/registry.rs`)
Communicates with npm registry:
1. Fetches package metadata with caching
2. Resolves version ranges using semver
3. Falls back to `latest` tag if range not matched
4. Retries failed requests with backoff

## Comparison with npm/yarn

| Feature | rnpm | npm | yarn |
|---------|------|-----|------|
| Concurrent Resolution | ✓ (30 workers) | ✓ | ✓ |
| Metadata Caching | ✓ | ✓ | ✓ |
| Progress Updates | ✓ (real-time) | ✓ | ✓ |
| Lockfile | ✓ (rnpm.lock) | ✓ (package-lock.json) | ✓ (yarn.lock) |
| Implementation | Rust | JavaScript | JavaScript |

## Limitations

- No peer dependency resolution yet
- No workspace/monorepo support
- Limited to public npm registry
- No offline mode

## License

MIT

## Contributing

Contributions welcome! Please feel free to submit issues and pull requests.
