# Netherica v0.1

Netherica is a Rust desktop ingestion/reporting application (egui/eframe) for Excel-based inventory flow.

## Runtime behavior

- Loads `config.toml` (auto-generates a default config on first run).
- Uses SQLite (`state.db`) with WAL mode.
- Writes archived source files to `archive/` and generated HTML reports to `reports/`.
- Embeds report assets using `rust-embed` from `asset/`:
  - `templates/report.html.tera`
  - `Sarabun-Regular.ttf`

Because assets are embedded at compile time, distribution binaries do **not** require an external `asset/` folder at runtime.

## Local development

```bash
cargo test --locked
cargo run --locked
```

## Build & distribution

The repository includes scripts to produce release artifacts in `dist/`:

- Windows (MSVC, static CRT): `scripts/build-windows-msvc.ps1`
- Linux (musl, static): `scripts/build-linux-musl.sh`

### 1) Windows MSVC static binary

PowerShell:

```powershell
./scripts/build-windows-msvc.ps1
```

Output:

- `dist/windows-msvc/netherica.exe`
- `dist/windows-msvc/SHA256SUMS.txt`

### 2) Linux musl static binary

Bash (Linux host):

Prerequisites (required for reproducible musl builds):

1. Install Rust + target:

```bash
rustup toolchain install stable
rustup target add x86_64-unknown-linux-musl
```

2. Install musl C toolchain providing `x86_64-linux-musl-gcc`:

- Debian/Ubuntu: `sudo apt-get install -y musl-tools`
- Fedora: `sudo dnf install -y musl-gcc`
- Alpine: `sudo apk add musl-dev musl-tools`

```bash
chmod +x scripts/build-linux-musl.sh
./scripts/build-linux-musl.sh
```

Output:

- `dist/linux-musl/netherica`
- `dist/linux-musl/SHA256SUMS.txt`

## Reproducible build guidance

Use the same Rust channel and locked dependencies:

- Recommended linker config is included in `.cargo/config.toml`:
  - target: `x86_64-unknown-linux-musl`
  - linker: `x86_64-linux-musl-gcc`

```bash
rustup toolchain install stable
rustup target add x86_64-unknown-linux-musl
cargo build --locked --release --target x86_64-unknown-linux-musl
```

```powershell
$env:RUSTFLAGS = "-C target-feature=+crt-static"
cargo build --locked --release --target x86_64-pc-windows-msvc
```

Then compare checksums with `SHA256SUMS.txt`.

## CI distribution workflow

GitHub Actions workflow: `.github/workflows/build-distribution.yml`

- Builds Windows MSVC static CRT binary.
- Builds Linux musl static binary.
- Uploads both artifacts with SHA-256 checksum files.
