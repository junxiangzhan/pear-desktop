<div align="center">

# :pear: Pear Tauri

[![GitHub release](https://img.shields.io/github/v/release/junxiangzhan/pear-desktop?include_prereleases&style=for-the-badge)](https://github.com/junxiangzhan/pear-desktop/releases)
[![Check status](https://img.shields.io/github/actions/workflow/status/junxiangzhan/pear-desktop/tauri-check.yml?branch=master&style=for-the-badge)](https://github.com/junxiangzhan/pear-desktop/actions/workflows/tauri-check.yml)
[![License](https://img.shields.io/github/license/junxiangzhan/pear-desktop?style=for-the-badge)](license)

</div>

A desktop and Android client for YouTube Music built on [Tauri v2](https://tauri.app/). It started as a port of [Pear Desktop](https://github.com/pear-devs/pear-desktop) and runs its renderer and plugins inside the page, with the Electron main process rewritten in Rust.

> [!NOTE]
> **Unofficial and independently maintained.** Upstream Pear Desktop declined to move to Tauri, so this fork goes its own way: Electron is being removed step by step and Tauri becomes the core. Please report issues [here](https://github.com/junxiangzhan/pear-desktop/issues), not to upstream.

> [!IMPORTANT]
> **No affiliation.** This project is not affiliated with, authorized by, endorsed by, or in any way officially connected with Google LLC, YouTube, or any of their subsidiaries or affiliates. "Google" and "YouTube Music" are trademarks of their respective owners and are used here for identification only.
>
> The software is provided "AS IS", without warranty of any kind; you use it at your own risk.

## Platforms

| Platform | Status |
| --- | --- |
| Windows | Installer on the [releases page](https://github.com/junxiangzhan/pear-desktop/releases) (not code-signed, so SmartScreen may warn) |
| Android | arm64 APK on the [releases page](https://github.com/junxiangzhan/pear-desktop/releases); GeckoView by default so Google sign-in works |
| macOS, Linux | Not built yet. They would offer the system WebView or GeckoView like Android; help testing is welcome |

## Documentation

Everything about the port lives in `tauri-poc/` (the folder keeps its name for now):

- [README (中文)](tauri-poc/README.md) · [README (English)](tauri-poc/README-en.md): architecture, plugin status, running on Windows and Android, releases
- [GeckoView plan](tauri-poc/GECKOVIEW-PLAN.md)

Quick start on Windows:

```bash
pnpm install --frozen-lockfile
pnpm exec vite build -c tauri-poc/vite.inject.config.mts
cargo run --manifest-path tauri-poc/src-tauri/Cargo.toml
```

## Translation

The UI strings come from upstream Pear Desktop, translated on [Hosted Weblate](https://bit.ly/48n5YF7).

## License

MIT, see [license](license). Based on [Pear Desktop](https://github.com/pear-devs/pear-desktop) by th-ch and the pear-devs contributors.
