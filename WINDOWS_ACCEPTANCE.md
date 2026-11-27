# Unsigned Windows x64 preview acceptance

This is an experimental portable build, not a signed or hardware-accepted release. Extract into a new local folder. Windows 10/11 x64 and Windows PowerShell 5.1 are required. Keep the journal and catalog on local storage; use an NTFS image output folder with sufficient free space. Do not bypass security warnings without independently verifying provenance and SHA256SUMS.txt. No installer or administrator elevation is requested by the package.

## Redesigned interface acceptance (Phase 8A-R4)

Repeat package checksum and startup checks for the newly downloaded ZIP, even if the original preview passed.
- Confirm the release tag identifies the tested commit and the external ZIP SHA-256 matches; extract in a new folder, then verify internal SHA256SUMS.txt.
- Check the overview, grouped navigation, Project menu, status area and every workspace in both themes. Resize to the minimum supported window and maximize; check normal and elevated Windows display scaling. Long paths must remain readable and controls reachable by scrolling.
- Use disposable fixtures: drag/drop files and folders, browse, enter a typed path, remove a selection, scan with filters and checksums, and change a source. Old inventory must be marked stale; rescan before planning. Existing plans must protect source selections.
- Compare strategies, review disc and folder views, lock/unlock, select a file and move it through an explicit destination. Locked or too-small destinations must reject a move. Review unassigned files; no automatic omissions.
- Save a new project revision, confirm existing files cannot be overwritten, export a plan without clearing the project dirty indicator, reopen a saved project, and check close/cancel behavior.
- Check catalog search and pagination, no-results states, filters, and expandable paths/checksums. A planned assignment is not evidence of writing.
- Check image preparation and verification controls, device refresh, assignment preview, journal selection, and confirmation reset after navigation. No actual recording during this stage.
- Report clipping, unclear labels, unexpected disabled controls, layout problems or confusing workflow. Include screenshots and exact steps. CI headless layout tests do not establish visual or interactive acceptance.

## Non-writing checks
- Verify each listed SHA-256 using Get-FileHash. Run disc-archive.exe --help and launch disc-archive-studio.exe.
- Use disposable fixture copies, not original archives. Exercise drag/drop, scan, planning, locks, project save/reopen and catalog search.
- Repeat with Unicode, long paths, mapped drives and UNC shares; confirm source files remain unchanged.
- Create and verify a UDF image locally; confirm collisions do not overwrite existing outputs. Check insufficient storage and unavailable shares fail safely.
- Discover recorders and preview an image; confirm wrong recorder, wrong hash, nonblank and unsupported media are rejected.

## Physical checks (operator only)
- Use expendable blank write-once media, a dedicated journal and an explicitly selected recorder. Review exact image hash and capacity before authorizing WRITE-ONCE.
- Observe stage and sector progress; 100% data progress is not finalization. Verify the completed disc using the journal-bound manifest.
- Check catalog linkage and historical readback status. A failed readback must not appear as a successful current verification.
- With two independent recorders and distinct images, test concurrent jobs and failure isolation.
- If interrupted, do not retry automatically. Confirm the helper stopped, inspect and remove suspect media, then explicitly reconcile the journal. Never create a new journal to bypass an unresolved attempt.

Record Windows version, drive models, media types, package hashes, results and errors. Do not claim hardware acceptance until these checks pass.

The CI package assembly checks and native file-stream tests do not exercise a real optical write. Confirm the selected Windows installation can complete the guarded ISO verification and native burn/readback sequence using disposable media before trusting it with archive material.
