# Public repository transition - publication hold

The owner requested public visibility to use free standard GitHub-hosted CI while retaining control over original material. This checkpoint is preparation, not approval that public release compliance or privacy review is complete.

Before changing visibility:
1. Review all reachable history (including tags), commit author metadata, issue/PR contents, Actions logs and artifacts, release descriptions/assets, and private workflow references. A current-tree pattern scan is not a complete secret audit. Rotate any discovered credential before public exposure. Do not rewrite history, delete artifacts, or change billing without separate authorization.
2. Review the original project reserved-rights and evaluation terms; they are not a guarantee of enforceability or of copyright in AI-generated material. Consult qualified counsel where needed. Public GitHub viewing/forking rights and applicable law remain unaffected.
3. Protect existing private preview binaries from public exposure until their third-party license review is complete. Preserve them, e.g. make the existing release a nonpublic draft if supported and confirm visibility, or retain the repository privately until its assets are cleared.
4. Review exact Windows build dependencies, transitive and embedded asset licenses, complete required notices and other obligations, and verify the package. THIRD_PARTY_NOTICES.md currently records outstanding work, not completed compliance.
5. Change only this repository's visibility through its Settings > General > Danger Zone after the preceding exposure checks are satisfied. No connector administration operation is available. Existing release workflow deliberately refuses public publication; maintain that gate until public binary compliance is complete.
6. After public conversion, manually run standard Windows CI on main; do not request Actions artifact upload. Only after compliance review and passing tests, adapt the release workflow to allow explicit owner-approved public prereleases with license materials and SHA-256 validation. No billing changes.
7. Keep other repositories private and automations paused unless the owner explicitly requests resumption.

Public visibility allows other users to copy/fork within GitHub under GitHub Terms of Service; copies may survive a later change to private. Never rely on a license notice as a mechanism to conceal secrets or prevent all copying.

## Public conversion reported and verified
Repository is now public. The original preview tag preview-96e0ec3dc921-45907196171-1 remains published and accessible; recommend owner make it a draft pending review if GitHub permits. No release deletion or alteration performed. Historical exposure audit and dependency clearance remain outstanding. Continue public standard-runner CI without artifact upload. The new license inventory step is a review aid, not a publication approval. Keep the existing private-only release workflow blocked for public repositories until completed notices and release gates are implemented. Only Disc Archive Studio automation may be resumed; leave other tasks paused.

Public standard CI 45946699269 passed, artifact upload skipped. Recovered lockfile SHA-256 verified against runner output; pinned lockfile and candidate inventory committed. Public distribution gate remains closed pending full notice collection and assessment of 11 missing legal-file candidates and font/embedded/native obligations. Historical audit still incomplete.

Pinned CI 45947538307 passed. Added upstream egui and font notices from exact tagged source plus hash-checked registry notice collection and missing-crate provenance logging. Public CI artifact upload now disabled even on manual input. Collection remains review-only; no public binary authorization until remaining upstream and embedded/native review, notice packaging and gate verification. Historical exposure audit remains outstanding.

CI 45948273940 passed, including 317 hash-validated notices, and all 54 tests. Next increment maps all 11 missing registry license candidates to exact upstream VCS commits and preserves five additional upstream texts. New CI must verify 322 notice copies and all 11 mappings. Remaining work: font name/copyright records, generated Khronos binding and native SQLite attributions, final notice bundle and release gate; historical exposure review. Public distribution remains blocked.

CI 45957870828 verified 322 notice copies, all 11 exact missing-notice mappings, 54 application tests and ZIP validation. Added embedded/native/generated attribution evidence collection after locked release build, with parser self-tests. Next: review actual extracted font names/licenses, SQLite headers and Khronos registry/generated output, then build and validate final distributable notice bundle. Historical audit and public release gate remain open; no public binary authorized.


CI 45959680920 at 31d90b71c3e1b67fdd1dddf29a7e1aa503ab5b69 failed after successful locked tests and release build: embedded evidence parser required a decoded copyright name record for every font. Review-only collection now records decoded, absent, or present-but-empty/undecodable status per exact-byte-verified font, preserving raw records and structural validation. Added synthetic regression coverage; new Windows CI pending. Missing metadata is not clearance: require explicit upstream copyright/notice review before any distribution. Public release remains blocked; historical audit outstanding.

CI 45975739797 at 0577a15daa38839b1553595b6acd2e224217ae26 failed in embedded attribution inspection with SQLite public-domain header not found (job 140426878305). Collector now searches bounded intact early comment blocks for explicit public-domain or SQLite copyright-disclaimer wording, retains the matching text and identifies the missing file on failure. Added synthetic regression cases. New CI pending; actual dedication evidence requires review, not automatic distribution clearance. Publication and historical audit remain outstanding.


CI 46018629677 at 8e4a127547f17fa7901367f113c436245d930629 failed in the SQLite dedication self-test (job 140572167483): repeated asterisk comment decoration prevented matching a wrapped public-domain declaration. Normalize repeated line-leading asterisks and CRLF for matching only; preserve original comment evidence. Added wrapped disclaimer regression coverage. Windows CI pending; publication and historical audit remain blocked.
