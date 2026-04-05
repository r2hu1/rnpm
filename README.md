# rnpm - A Fast Rust-based Node Package Manager

A high-performance package manager for Node.js projects, written in Rust. Features concurrent dependency resolution and parallel downloads for significantly faster installation times.

## Features

- **Concurrent Dependency Resolution**: Resolves up to 30 packages simultaneously using parallel async workers
- **Peer Dependency Resolution**: Automatically detects and resolves peer dependencies with real-time feedback
- **External Lock File Support**: Import and use lock files from npm, yarn, pnpm, or Bun with automatic detection
- **Smart Configuration**: Save lock file preference in `rnpm.config.json` for automatic use
- **Smart Deduplication**: Each package is fetched only once, even if multiple dependencies require it
- **Real-time Progress Tracking**: Live updates showing resolved packages and pending work
- **Metadata Caching**: In-memory cache prevents redundant API calls
- **Retry Logic**: Automatic retry (3 attempts) with exponential backoff for failed requests
- **Connection Pooling**: Optimized HTTP client with keep-alive and connection reuse
- **Lockfile Support**: Uses `rnpm.lock` for deterministic installs
- **Standard Commands**: Supports `install`, `update`, `add`, `remove`, `run`, and `import`

## Installation

### Quick Install (No Git Clone Required)

**macOS/Linux:**

```bash
curl -fsSL https://raw.githubusercontent.com/r2hu1/rnpm/main/scripts/install-standalone.sh | bash
```

Or download and run manually:

```bash
./scripts/install-standalone.sh
```

**Windows (PowerShell):**

```powershell
iwr -useb https://raw.githubusercontent.com/r2hu1/rnpm/main/scripts/install-standalone.ps1 | iex
```

Or download and run manually:

```powershell
.\scripts\install-standalone.ps1
```

### Manual Installation from Source

If you prefer to build from source:

```bash
git clone https://github.com/r2hu1/rnpm.git
cd rnpm
cargo build --release
cp target/release/rnpm ~/.local/bin/  # or any directory in your PATH
```

### Verify Installation

```bash
rnpm --version
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

### Import Lock File

```bash
rnpm import [lockfile-path]
```

Imports dependencies from external lock files. Auto-detects if path not specified.

**Supported Formats:**

- ✅ npm (`package-lock.json` v2+)
- ✅ Yarn (`yarn.lock`)
- ✅ Bun (`bun.lock` JSON format)
- ✅ pnpm (`pnpm-lock.yaml`)

## Configuration

### rnpm.config.json

Create an `rnpm.config.json` file to customize behavior:

```json
{
  "useLockfile": "npm"
}
```

Options for `useLockfile`:

- `"npm"` - Use `package-lock.json`
- `"yarn"` - Use `yarn.lock`
- `"pnpm"` - Use `pnpm-lock.yaml`
- `"bun"` - Use `bun.lock` or `bun.lockb`
- `null` - Use `rnpm.lock` (default)

When you run `rnpm install` without an existing `rnpm.lock`, it will automatically detect external lock files and ask if you want to use them. Your choice is saved to `rnpm.config.json` for future use.

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

| Feature               | rnpm           | npm                   | yarn          |
| --------------------- | -------------- | --------------------- | ------------- |
| Concurrent Resolution | ✓ (30 workers) | ✓                     | ✓             |
| Metadata Caching      | ✓              | ✓                     | ✓             |
| Progress Updates      | ✓ (real-time)  | ✓                     | ✓             |
| Lockfile              | ✓ (rnpm.lock)  | ✓ (package-lock.json) | ✓ (yarn.lock) |
| Implementation        | Rust           | JavaScript            | JavaScript    |

## Limitations

- No workspace/monorepo support yet
- Limited to public npm registry
- No offline mode
- Peer dependency version validation is informational only (doesn't enforce strict compatibility)

## License

MIT

## Contributing

Contributions welcome! Please feel free to submit issues and pull requests.
