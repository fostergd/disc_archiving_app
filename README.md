# Disc Archive Studio

Phase 3A development preview: read-only inventory and estimated disc planning via CLI and desktop GUI. No optical writing yet. Windows 10/11 x64 target.

All regular file types are included by default. No source deletion, transcoding, recovery data, incremental archive, rewritable management, disc copying, label printing, periodic audits, multisession, audio CD or movie authoring.

CLI:
- disc-archive media
- disc-archive scan "D:\\Videos" --output inventory.json --hash
- disc-archive plan inventory.json --output plan.json --mode hybrid
- disc-archive duplicates inventory.json

Use --extension repeatedly for an allowlist and --exclude for archive-relative glob patterns. Hashing is optional and streams source files. Duplicate reports never remove files. Outputs must not already exist. Keep reports outside source trees. Reports contain local paths and should not be committed to Git.

Default 50 GB Blu-ray DL; --capacities accepts comma-separated capacities in bytes, with the final capacity repeating. Provisional overhead is stored in new plans. Actual image size and writable capacity must be validated later. Folder/hybrid modes group recursive folder trees and matching audio/video sidecars when possible. Numeric sorting covers digit runs anywhere in the path. No media-specific parsing yet. Unicode paths use native Rust filesystem APIs, but optical compatibility, UNC and long path acceptance remain pending. Symbolic links and Windows reparse points (including junctions) are excluded by default; overlapping canonical source paths are collapsed to their parent selection. Directory exclusion matches prune traversal. Exclusion globs are case-insensitive and use slash-separated archive paths; use ** for recursive matching. Drive and UNC share roots receive named archive prefixes. Long Unicode path and junction tests run in Windows CI. Real network shares and mapped drives still need acceptance testing. Empty folders are not represented. Archive path collisions fail rather than rename silently.

Next: filesystem edge-case validation, scalable project persistence, GUI with drag/drop, SQLite catalog, UDF image generation, verified single-session write-once burning, existing image burning and concurrent independent drives. CLI and GUI share the library.

Phase 2B: compare inventory.json, validate plan.json, edit plan.json --output revised.json lock 1, edit plan.json --output revised.json move "Videos/Movie.mkv" 2, edit plan.json --output revised.json rename "VIDEO_{n:03}", and edit plan.json --output revised.json add-disc 50000000000. Every edit creates a new file. Plans generated before Phase 2B should be regenerated to include the reserve and labels. Locked discs cannot receive or release files and retain labels. Replanning preserves locked disc assignments, identifiers, capacities and labels. Labels are conservative ASCII identifiers up to 32 characters, not validated optical volume names yet.

Phase 2C: edit plan.json --output revised.json replan --capacities 25000000000,50000000000 --mode hybrid --pattern "VIDEO_{n:03}". Capacities apply only to unlocked files and the existing reserve remains unchanged. Previously unassigned files are reconsidered; locked discs retain their exact records. Unlocked disc numbers may change. A naming collision fails validation rather than overwriting a locked label.

Phase 2D.2: natural sorting caches folded keys per sort; first-fit packing uses an indexed free-capacity tree (logarithmic lookup/update, amortized growth), preserving first-fit order. CI exercises 50,000 synthetic file entries in every strategy and checks indexed lookup against a linear reference. Inventory and plan metadata still reside in RAM; no claim of bounded-memory million-file operation or real disk throughput is made.

GUI preview: cargo run --features gui --bin disc-archive-studio. Windows CI builds both executables. Source selection supports drag-and-drop, folder/file dialogs and typed UNC paths. Scan, plan, compare and replan use background workers. The planner supports locks, manual moves, naming and JSON import/export; outputs cannot overwrite existing files. GUI runtime, drag/drop and usability still require Windows desktop acceptance. No physical optical functionality is exposed yet.

Phase 3B: searchable full inventory, exclusion browsing, report-only SHA-256 duplicate groups, unsaved-plan close confirmation, worker-based plan export, and safeguards against replacing an open plan. Loaded plans are conservatively marked unsaved. GUI behavior requires interactive Windows acceptance; duplicate detection never deletes or excludes files automatically.

Phase 4A: versioned .das.json project snapshots store source selections, filters, hashing preference, capacity settings, inventory, and plan including locks and assignments. Save Project As writes a new revision without replacing an existing file; temporary sibling files are flushed before publication. Open Project requires an empty workspace. Projects validate inventory/plan agreement and reject unknown versions. CLI: project-pack inventory.json plan.json --output archive.das.json, project-check archive.das.json, project-extract archive.das.json --plan restored.json. Project paths are local/private. Burn progress and SQLite catalog integration remain future work.

Phase 4B.1: local bundled SQLite catalog, with transactional imports of validated project snapshots, project listing and paginated literal substring searches across archive paths and stored hashes. `catalog-create archive.sqlite`, `catalog-import archive.sqlite archive.das.json`, `catalog-projects archive.sqlite`, `catalog-search archive.sqlite Movie --limit 100 --offset 0`. Each import creates a distinct snapshot, not an update or duplicate-removal operation. Assignments are explicitly planned/unassigned, never evidence of burning. Source access is unnecessary for catalog searches. Catalog GUI and physical verification status are subsequent increments. SQLite is bundled, so no separately installed database server is required. Catalogs contain private source paths.
