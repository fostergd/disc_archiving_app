# Third-party license review

Disc Archive Studio's reserved-rights terms apply only to the original project material to the extent rights exist. They do not supersede licenses for third-party code, libraries, fonts, tools, or other materials.

Direct dependencies declared in Cargo.toml: anyhow, clap, globset, rusqlite (bundled SQLite), serde, serde_json, sha2, tempfile, walkdir; optional desktop dependencies eframe and rfd. Additional transitive dependencies, platform-specific components, and embedded fonts may be included in the executable. Windows system components are governed by their respective terms.

**Release compliance review outstanding.** This list is an inventory, not a complete license notice or a representation that all obligations have been satisfied. Before any public binary distribution, resolve and record exact dependency versions for the Windows x64 GUI and CLI builds, review all applicable license expressions and source materials (including build-time embedded assets and fonts), collect required license texts, copyright and attribution notices, and determine whether any source-distribution or other obligations apply. Include the resulting complete notices and any required accompanying material in the ZIP. Do not rely on SPDX identifiers alone. Do not publish a public binary if any required notice, permission, or other condition remains unresolved. Retain upstream license texts unchanged.

Existing private preview assets predate this notice and have not been independently audited for public distribution. Keep them private or in a nonpublic draft pending review; do not delete them as part of the transition.
