# Embedded attribution review - CI 46064755192

Scope: pinned Windows GUI/CLI build at 9232d34f6d9a343c304b4d4a7e4864b3c5399525. Evidence retained in embedded-evidence-baseline.json. This review is not a blanket compliance determination or approval to publish.

## Fonts

All four font blobs matched the exact egui 0.31.1 upstream commit. No font binaries are redistributed separately in this review. The already collected upstream notices must accompany the executable.

- Hack-Regular: Source Foundry Authors (2018), Bitstream Inc. (2003); MIT and Bitstream Vera terms, DejaVu public-domain contributions. Preserve Hack-Regular.txt and embedded license/copyright records.
- NotoEmoji-Regular: Copyright 2013 Google Inc. All Rights Reserved.; Noto trademark of Google Inc.; SIL OFL 1.1. Preserve OFL.txt and the observed copyright/trademark records.
- Ubuntu-Light: Copyright 2011 Canonical Ltd. Licensed under Ubuntu Font Licence 1.0; Ubuntu and Canonical registered trademarks. Preserve UFL.txt and the observed copyright/trademark records.
- emoji-icon-font: no decoded copyright name record present. Exact upstream egui font blob matches, and its associated emoji-icon-font-mit-license.txt identifies Copyright (c) 2014 John Slegers. Preserve that original MIT notice; no claim that missing embedded metadata grants rights.

Do not modify fonts or sell them separately; do not imply endorsement or grant third-party trademark rights. Font-specific terms continue to apply.

## SQLite

The locked libsqlite3-sys 0.30.1 feature set includes bundled and bundled_bindings; SQLCipher features absent. Both bundled sqlite3.c and sqlite3.h contain author copyright-disclaimer/dedication comments. Preserve the exact extracted records and the Rust wrapper license. The separate SQLCipher license collected conservatively from the crate is not evidence that SQLCipher is linked. Review other compiled native-code and source-header obligations before distribution.

## Generated bindings

Actual build-script events yielded egl_bindings.rs, wgl_bindings.rs and wgl_extra_bindings.rs. Their leading legal-comment arrays were empty. The Khronos input scan reported 66 XML/IDL files, including two WebGL IDL notices. A leading-comment scan is insufficient for XML registry data: notices may be within <comment> elements. Extend collection to those elements and inspect resulting evidence before release. Preserve gl-rs upstream Apache terms and all relevant registry notices; no inference of permission from empty arrays. Review which registry inputs contribute to generated/native code.

## Open items

Review the improved XML evidence, other source-header/native obligations and exact Windows runtime/build candidates; assemble a complete notice bundle, validate its contents, finish historical exposure review and public release gate. The existing legacy public preview predates this review. No public binary authorization yet.

## Expanded XML evidence - CI 46065482256
The verified XML scan identified Khronos copyright and Apache-2.0 notices in GL/GLX/WGL, Khronos permissive notices in EGL, and ANGLE BSD references in egl_angle_ext.xml/gl_angle_ext.xml. Exact extracted text is retained in embedded-evidence-xml.json. The gl-rs commit f150967b1c44ae888e6676f93f639ebc82771bdc pins google/angle submodule 7403dd2cd3764fe96660fe09892e764e9ae1dbca; original ANGLE LICENSE and provenance retained. The candidate bundle preserves all 322 verified registry/upstream texts, exact embedded report, review document and ANGLE license/provenance. This is conservative review evidence, not proof of complete source-header/native compliance. Historical audit and final distribution review remain open.
