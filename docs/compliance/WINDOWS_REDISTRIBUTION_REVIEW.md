# Windows redistribution declaration review

Reviewed at b5307d3405d3869da499b355d24f6142e8d1cf56; Windows CI 46118161173 passed. Scope is the exact pinned Windows x64 GUI/CLI candidate graph, not all lockfile platforms or a linked-binary SBOM.

## Declaration assessment

All 174 candidates are accounted for in windows-redistribution-index.json: 162 compiled candidates and 12 inventory-only candidates. The declared expressions contain permissive alternatives or permissive conjunctions; the font-specific conjunction requires independent OFL and Ubuntu Font Licence compliance. Proposed terms select MIT for explicit MIT alternatives and BSD-3-Clause for the two BSD/Apache alternatives. Keep all original texts and notices, including unselected alternatives and component-specific notices. Eight historical slash-style expressions retain both texts pending verification of the upstream alternative wording; do not silently reinterpret them. No reciprocal source-disclosure requirement is identified from the application candidate declarations alone. This is a technical review, not legal advice or final license clearance.

Obligations for proposed terms: MIT/ISC retain original copyright and permission/disclaimer; BSD-3-Clause retain notices/disclaimer in binary accompanying material and avoid endorsement; Apache-2.0 include license and applicable attribution/NOTICE, mark modified files where applicable and preserve relevant notices; Boost retains applicable source notices (object-code exception does not require removal); Unicode retain applicable notice; Zlib retain source notice, do not misrepresent origin and mark alterations; OFL and Ubuntu Font Licence preserve font-specific copyright/license terms and naming restrictions. Keep all fonts unchanged and do not sell separately. Preserve original font notices, SQLite dedication, Khronos/ANGLE notices and official Rust distribution reports. No Microsoft runtime DLLs distributed.

The index maps every candidate to exact already-collected legal file paths, including all 11 missing standalone mappings and the four embedded fonts. Bundle verification now rejects missing or misassigned paths and copies the reviewed index byte-for-byte. Existing 348 payloads retained, one navigation index added. CI must confirm the 349-file candidate.

## Specific remaining work

- Confirm the slash-style upstream license declarations and review copyright/NOTICE text coverage, especially Apache-only packages, embedded native/generated sources and any component-specific conditions. Exact collected texts are preserved, but file identity is not itself a review of every required notice.
- Review the official toolchain copyright report and target-specific exclusions. SGX/UEFI candidates are report-only, not inferred to be Windows-linked; preserve original reports and license texts.
- Resolve the binary email-shaped finding, legacy preview and explicit owner authorization separately. Do not treat this declaration review as permission to publish.

No distribution approval; hold remains intact.
