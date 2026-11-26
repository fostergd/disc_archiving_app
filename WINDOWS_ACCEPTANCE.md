# Unsigned Windows x64 preview acceptance

This is an experimental portable build, not a signed or hardware-accepted release. Extract into a new local folder. Windows 10/11 x64 and Windows PowerShell 5.1 are required. Keep the journal and catalog on local storage; use an NTFS image output folder with sufficient free space. Do not bypass security warnings without independently verifying provenance and SHA256SUMS.txt. No installer or administrator elevation is requested by the package.

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
