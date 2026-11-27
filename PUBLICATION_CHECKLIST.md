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
