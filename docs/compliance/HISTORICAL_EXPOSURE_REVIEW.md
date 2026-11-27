# Historical exposure review - initial checkpoint

Source: repository API inspection at main da76ec81aa585fe62459e91e93fd0257c27a2917, and successful Windows run 46066997607.

- Branch listing: one branch, main. Issue and pull request lists (all states, first page of 100): zero each.
- Release listing: one published legacy prerelease, preview-96e0ec3dc921-45907196171-1, with two assets. Existing binary and checksum predate the current review. No release change or deletion performed.
- Tag listing and repository-wide Actions artifact listing were attempted but rejected by the available connector. Full-history CI checkout now fetches branches and tags for reachable object screening. This does not substitute for review of Actions artifacts or deleted/inaccessible refs.
- Current CI succeeded; no artifact upload in the latest run. Historic logs and artifacts, release asset contents and GitHub settings have not been comprehensively reviewed.
- New script audit-reachable-history.py scans unique fetched reachable Git objects, including blobs, trees, commits and tags, under explicit bounds. It reports counts only, including non-noreply commit-email counts; no credential values, email values, paths or matched strings are printed. A high-confidence match or incomplete scan fails closed. Patterns are heuristic, not proof of absence of secrets. Review-pattern counts may include legitimate public upstream attribution, version strings and synthetic tests.
- CI result is pending. If possible credential exposure is detected, privately inspect and rotate affected credentials; never expose match contents in public logs, rewrite history, delete artifacts, or change visibility without separate authorization.

Remaining: inspect the actual history screen results and resolve findings, review available historic Actions logs/artifact listings and release content as feasible, and record explicitly inaccessible scope before deciding release readiness. The original preview remains public and unaudited; the recommendation to return it to draft if supported remains outstanding.

No publication authorization is granted by a screening pass.

## Verified screening checkpoint
Run 46068498978 at 17750fe08be4499563414df06be8037f6fd90305 passed. 542 unique objects (285 blobs, 65 commits, 192 trees), 3,258,574 bytes, one head, one remote-tracking branch, one tag, no skipped objects, zero high-confidence pattern matches, zero distinct non-noreply commit email addresses. Review-pattern object counts: 66 email, one local user path. These need classification, not automatic clearance. Actions listing returned 65 runs; four representative logs (jobs 140206661574, 140193851590, 138991780467, 138995288662) screened for private key headers, GitHub/AWS tokens, private IPs and emails, no matches. Job 140318920041 unavailable (BlobNotFound). Remaining historical logs are not yet screened. Legacy release run 45907196171 lists no Actions artifacts; release API confirms two asset digests, not an inspection of asset bytes. No release changes. Continue review of flagged patterns and remaining historical material.

## Pinned toolchain CI checkpoint
Run 46078156707 at 690df74e3f1071099f5f86ab07ef4e1d714cd1df passed: 584 reachable objects (308 blobs, 67 commits, 209 trees), 3,482,401 bytes, no skipped objects, no high-confidence matches, zero distinct non-noreply commit email addresses. Review-pattern object counts: 68 email, one local-user-path. Added counts-only classification of noreply author/committer headers, exact independently obtained upstream notice blob identities, exact self-matching local-path regex literal and unclassified matches. No exclusion from the original pattern scan; no match values or object paths emitted. New CI pending. Remaining historical logs and legacy asset contents are not fully reviewed.
