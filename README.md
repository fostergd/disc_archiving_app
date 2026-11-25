# Disc Archive Studio

Phase 2 development preview: read-only inventory and estimated disc planning. No optical writing or GUI yet. Windows 10/11 x64 target.

All regular file types are included by default. No source deletion, transcoding, recovery data, incremental archive, rewritable management, disc copying, label printing, periodic audits, multisession, audio CD or movie authoring.

CLI:
- disc-archive media
- disc-archive scan "D:\\Videos" --output inventory.json --hash
- disc-archive plan inventory.json --output plan.json --mode hybrid
- disc-archive duplicates inventory.json

Use --extension repeatedly for an allowlist and --exclude for archive-relative glob patterns. Hashing is optional and streams source files. Duplicate reports never remove files. Outputs must not already exist. Keep reports outside source trees. Reports contain local paths and should not be committed to Git.

Default 50 GB Blu-ray DL; --capacities accepts comma-separated capacities in bytes, with the final capacity repeating. Provisional overhead is stored in new plans. Actual image size and writable capacity must be validated later. Folder/hybrid modes group recursive folder trees and matching audio/video sidecars when possible. Numeric sorting covers digit runs anywhere in the path. No media-specific parsing yet. Unicode paths use native Rust filesystem APIs, but optical compatibility, UNC and long path acceptance remain pending. Symbolic links are skipped; junction handling needs Windows tests. Empty folders are not represented. Archive path collisions fail rather than rename silently.

Next: locked-disc replanning, scalable project persistence, GUI with drag/drop, SQLite catalog, UDF image generation, verified single-session write-once burning, existing image burning and concurrent independent drives. CLI and GUI share the library.

Phase 2B: compare inventory.json, validate plan.json, edit plan.json --output revised.json lock 1, edit plan.json --output revised.json move "Videos/Movie.mkv" 2, edit plan.json --output revised.json rename "VIDEO_{n:03}", and edit plan.json --output revised.json add-disc 50000000000. Every edit creates a new file. Plans generated before Phase 2B should be regenerated to include the reserve and labels. Locked discs cannot receive or release files and retain labels. Replanning around locked discs is not yet implemented. Labels are conservative ASCII identifiers up to 32 characters, not validated optical volume names yet.
