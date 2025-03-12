# fswatch

Cross-platform, high-performance directory watcher.

- Fast: Backended by `notify-rs`, will prefer fs event as possible.
- Portable: Built with bundled sqlite-sys backend, no external libraries needed.
- Lightweight: ~2M executable size, ~1MiB RAM usage. (on Windows MSVC x86_64)
- Simple: Only two parameters, one for directory, one for sqlite database.

Made with ❤ and DeepSeek-R1.
