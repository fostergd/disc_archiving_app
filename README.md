# Disc Archive Studio

Phase 1 development preview: read-only inventory and estimated disc planning. No optical writing or GUI yet. Windows 10/11 x64 target.

All regular file types are included by default. No source deletion, transcoding, recovery data, incremental archive, rewritable management, disc copying, label printing, periodic audits, multisession, audio CD or movie authoring.

CLI:
- disc-archive media
- disc-archive scan "D:\\Videos" --output inventory.json --hash
- disc-archive plan inventory.json --output plan.json --mode hybrid
- disc-archive duplicates inventory.json

Use --extension repeatedly for an allowlist and --exclude for archive-relative glob patterns. Hashing is optional and streams source files. Duplicate reports never remove files. Outputs must not already exist. Keep reports outside source trees. Reports contain local paths and should not be committed to Git.

Current planner uses one capacity per plan; default 50 GB Blu-ray DL, with provisional overhead. Actual image size and writable capacity must be validated later. Folder/hybrid modes group immediate parent folders when possible; oversized folders fall back to individual files. Numeric sorting covers digit runs anywhere in the path. No media-specific parsing yet. Unicode paths use native Rust filesystem APIs, but optical compatibility, UNC and long path acceptance remain pending. Symbolic links are skipped; junction handling needs Windows tests. Empty folders are not represented. Archive path collisions fail rather than rename silently.

Next: recursive folder and sidecar grouping, mixed capacities, manual locks, scalable project persistence, GUI with drag/drop, SQLite catalog, UDF image generation, verified single-session write-once burning, existing image burning and concurrent independent drives. CLI and GUI share the library.
