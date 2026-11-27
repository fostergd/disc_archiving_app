# Disc Archive Studio — continuation checkpoint

Previous checkpoint head: e1f63982193d8297393276c00f5212e89aa0f6df (Windows CI 45728362650).
Current increment: 6F.2 GUI explicit catalog linkage and historical readback timestamp display, with synthetic catalog regression assertions. Parent 555739d881d52e98209fa42db95c3a9b1155ed12 passed Windows CI 45731443856. New CI pending.

Current increment 6G.1: strict ordered native writer lifecycle protocol and non-writing regression tests for missing, duplicate, out-of-order and unexpected markers. Parent a58e76248339b7e19580c8b2c6c55e0f79840fa2 passed Windows CI 45737912807. New CI pending. Parent CI 45750494175 passed.

Current increment 6H.2: journal creation uses the same foreign-key and busy-timeout settings as reopen; synthetic legacy migration and case-insensitive unresolved-attempt regression coverage. Parent 50170c3417aa9e0ee28880a633ed3fbfab8ff8dc passed Windows CI 45785400276. New CI pending.

Current increment 6H.3: reject prepared manifests whose aggregate payload overflows or exceeds usable capacity before native verification; synthetic boundary and overflow regression test. Parent bb61ed1345a305d0496044f2347f0cef0f351e0e passed Windows CI 45794379920. New CI pending. Output publication and storage review remain next.

Current increment 6H.4: validate output filenames and reject existing filesystem entries before staging; retain atomic no-replace hard-link publication. Synthetic output path regression tests added. Parent 3b00acd4114ddbdd88e3c295b150771b663d4e1f passed Windows CI 45813309486. New CI pending. Next: storage review and portable release preparation.

Current increment 6H.5: explicitly flush verified image before no-replace hard-link publication; synthetic regression covers destination collision and survival after staging cleanup. Parent 54cf1c59cba268cb5cb8f2dcc5e73729cd74a740 passed Windows CI 45831506197. New CI pending. Next: portable release preparation and acceptance checklist.

## Remaining work, in priority order
1. If current CI fails, inspect logs and fix. GUI and CLI linkage require exact pre-existing hashes and latest successful readback. Catalog status is historical, not a live media assertion.
2. Phase 6G.2 adds optional native IMAPI sector progress and synthetic tests (CI 45769776994 passed). Phase 6H.1 binds GUI batch preview authorization to ordered recorder identities and checksums, with regression tests; new CI pending. Next: GUI state/authorization review, durable recovery, migration tests, image path and storage robustness. Real callback delivery requires hardware acceptance.
3. Portable unsigned Windows release packaging, documentation and safe manual acceptance checklist (including GUI, mapped shares, real optical media and concurrent drives).
4. Stop for genuine hardware/user acceptance; never perform physical burns or modify source user files from automated development.

The CI pass is not hardware acceptance. On a green current HEAD and unfinished GitHub-only work, implement and commit a meaningful tested increment; a status-only report is not continuation. If blocked, identify the exact attempted action and error. Use compare-and-swap main updates and avoid racing active CI or other sessions. No artifact deletion or billing changes. Never claim a commit or CI result without fresh evidence.

Phase 7A: portable unsigned Windows x64 package assembly, SHA-256 manifest, CI package smoke and collision regression checks, and operator acceptance checklist. Parent 30d6e6788615ee0eb9d698da3bc42387d436ceaf passed Windows CI 45842442553. New CI pending; artifact upload remains opt-in. Next: validate package CI and review remaining release readiness before physical acceptance.

Phase 7B.1: Phase 7A parent b299316ca6dddf7841ba55fe7ea463e48231aaeb passed Windows CI 45852090628, including package integrity and packaged CLI checks; artifact publication intentionally skipped. This increment tests the actual native read-only COM stream against a disposable fixture (length, bytes, seek, EOF, concurrent read guard), and pins the documented SHCreateStreamOnFileEx entry point. New CI pending. If green, review final readiness and remaining hardware-only acceptance, then stop rather than repeatedly performing status-only scheduled checks. No physical operations in CI.

Phase 7B.2: parent 8970f0cfb10db471a9738966368553e9aa656d78 passed Windows CI 45861032404. Current preview README, package handoff instructions and CLI help now describe implemented capabilities and explicit physical acceptance limitations; historical notes retained as history. New CI pending. If green, GitHub-only work is complete for this preview; stop for operator package acquisition and Windows acceptance. Artifact upload remains opt-in.

Phase 7C: alternate private release publishing, requested after manual artifact run 45903256825 passed tests/build/package but failed at Actions artifact quota. Shared package validation and ZIP round-trip validation are in normal CI; separate manual private release workflow publishes verified ZIP and external SHA-256 sidecar through GitHub Releases. No physical operations, artifact deletion, repository visibility or billing changes. CI pending; manual release execution required to prove publication. Hardware acceptance remains outstanding.

## Phase 8A redesign supersedes prior stopping point
Operator accepted initial launch, CLI help/version, and package checksums. User requested a full UI usability redesign with resumed hourly GitHub-only continuation. Current R1 implementation and remaining R2–R4 stages are tracked in UI_REDESIGN.md. Do not stop at the previous Phase 7 acceptance gate; stop at the updated private portable preview readiness and Windows acceptance gate. Parent 96e0ec3dc92122ce9cc2462e174d9a8c6f01d0b0 passed CI 45906022568 and private release 45907196171. R1 CI pending.

Phase 8A-R1 parent 72aa10e610096b8fef20e2f4b5bea50b1bb3cafc passed Windows CI 45912233115. R2 source workflow and state protection implemented; new CI pending. Next R3a planner usability per UI_REDESIGN.md.

Phase 8A-R2 7ed2af0b47302d853967c4990c740d1d207dc125 passed Windows CI 45919724275. R3a planner, comparison and duplicate review redesigned; new CI pending. Next R3b per UI_REDESIGN.md.

Phase 8A-R3a 5a82b41f3d7fbf40d9300d12b6fb09fc3450579d passed Windows CI 45921114930. R3b optical workspaces redesigned with explicit stages and authorization reset safeguards; new CI pending. Next R3c per UI_REDESIGN.md.

Phase 8A-R3b 3e79e96462f6dfb6786fb36490b3e29933e22d33 passed Windows CI 45940393862. R3c catalog/project UX, search state, and narrow-window navigation implemented; new CI pending. Next R4 per UI_REDESIGN.md.

Phase 8A-R3c 7d790575a78ff487610695f023b7739494395303 passed Windows CI 45941462305. R4 final combined GUI review, non-interactive egui smoke tests, worker guard and revised Windows acceptance checklist implemented. Exact-head CI pending. On success, use existing private release workflow and verify publication before Windows acceptance. No hardware acceptance claimed.

## Publication preparation (CI intentionally skipped)
Owner requested public visibility due to exhausted GitHub Actions compute and storage allowance. All automations remain paused. Reserved-rights/evaluation terms, third-party review inventory, security and contribution guidance and publication checklist prepared. No visibility or billing change; existing release retained. Public distribution remains on hold pending history/log/asset exposure and dependency license reviews. Current R4 application commit 547337976b8fff5fd24c56e216bea9c0243290d0 was awaiting CI at pause; do not represent publication preparation as tested. No new build requested while private.

Public conversion verified. Parent 71748c174fc236e4400a311558d52c2bd2f2410e was committed with skip CI while private. Earlier R4 run 45942238607 failed; job 140318920041 has no recorded steps and logs return BlobNotFound. No evidence of a code failure versus a quota failure. This increment triggers full standard public Windows CI and adds dependency/asset/license inventory using locked resolution within the same build. No new release or artifact upload requested. Next: inspect exact CI and inventory output, pin Cargo.lock from reviewed resolution, complete upstream license materials, then adapt public manual release gate. Historical exposure audit remains outstanding; existing legacy release publicly visible.

Public CI 45946699269 at a404b2a9d618bcfdfcdf3f9c4d6580028bf70e53 passed 54 tests, release build and ZIP round-trip. Exact lockfile SHA-256 60862ef1ad2cfab8812323835ad07f16294ccd4b09b6a2b32b7ea5dc5cccc567 verified from CI output and pinned, 174 Windows dependency candidates recorded with legal-file hashes. New CI pending; next: collect upstream notice bytes and attribution from pinned dependencies, resolve missing legal files and font/native obligations; keep public publishing blocked until complete.
