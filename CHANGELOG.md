# Changelog

All notable changes to **Nullex Client** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.1.0] - 2026-11-14

### Added
- **Native Windows ARM64 Support**: Full native binary compilation for `aarch64-pc-windows-msvc` target alongside `x86_64-pc-windows-msvc`.
- Native ARM64 companion binaries including Mihomo v1.19.34 core, Wintun driver, and service helpers.
- Dedicated ARM64 NSIS installer and portable `.7z` distribution packages.
- Added connection streaming and active socket watchdog synchronization.

### Changed
- Improved named pipe communication stability between UI frontend and Mihomo core API.
- Refactored core management architecture to eliminate handle exhaustion during extended sessions.
- Enhanced TUN mode reliability with automatic routing table verification and graceful failover.
- Cleaned and streamlined internal component naming and packaging scripts.

### Fixed
- Fixed potential pipe deadlock under rapid proxy node switching.
- Resolved Windows shutdown interception to ensure clean adapter teardown without blocking system exit.
- Corrected sidecar detection paths in diagnostics and orphan process cleanup.

---

## [1.0.0] - 2026-11-12

### Added
- Initial public release of Nullex Client (Rust & Tauri 2 edition).
- High-performance Mihomo (Clash Meta) core integration.
- Full protocol support: VLESS (Reality), Hysteria 2, TUIC v5, Shadowsocks, WireGuard, and Trojan.
- Virtual network adapter (Wintun) with system-wide TUN mode.
- System proxy toggle with rule-based routing and bypass lists.
- Subscription manager with automatic hourly and daily sync.
- Real-time node latency benchmarking (TCP ping and HTTP handshakes).
- Dark theme interface with customizable dashboards and connection inspector.
- Bilingual interface support (`en-US` and `ru-RU`).
- In-app automatic update checker.
