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
