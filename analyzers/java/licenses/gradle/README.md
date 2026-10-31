# Gradle Tooling API license evidence

`manifest.json` covers Tooling API 8.14.3 and its eight shaded third-party components. Each referenced copyright header, full license text and applicable POM is retained with its source URL and SHA-256. Packaging verifies these files and copies them with the Java language pack. The separate Maven bundle covers standalone Maven integration dependencies.

## Gradle shaded evidence

Published Tooling API8.14.3 JAR contains **no LICENSE/NOTICE/COPYING resources**. It has 1290 relocated third-party classes (actual count is recorded by gradle-shaded-classes.txt). Source v8.14.3 tooling-api build uses the shaded-jar plugin, retains Gradle code and excludes SLF4J from shading. Exact-tag distribution constraints identify:

- ASM9.7.1, Commons IO2.15.1, Commons Lang2.6.
- Guava33.4.6-jre and its failureaccess1.0.3 dependency (also verified from Guava's exact-version POM).
- Error Prone annotations2.36.0, javax.inject1, JSR3053.0.2.

Actual relocated package/class list is retained in gradle-shaded-classes.txt. Gradle source evidence files are gradle-distributions-dependencies.gradle.kts, gradle-ExternalModulesExtension.kt, gradle-dependency-modules.gradle.kts. Their authoritative sources are https://github.com/gradle/gradle/tree/v8.14.3 with corresponding relative paths. Tooling API's own Apache LICENSE came from exact tag v8.14.3. No root NOTICE exists at that tag (HTTP404); do not invent one.

## Upstream licensing qualifications

- **JSR305 classification discrepancy remains explicit**. Gradle's own metadata calls it BSD3; the exact3.0.2 Central POM declares Apache2.0; the concurrent annotation sources declare Brian Goetz CC-BY2.5. Tooling API actually shades Immutable and ThreadSafe, so those attribution headers matter. Exact POM, original headers, Apache license and Creative Commons full legalcode (from the official Creative Commons GitHub source) are preserved. Do not flatten this to a single BSD label.
- Exact-version original source headers and canonical full texts together supply redistributable texts; they should be copied together, preserving provenance and copyright notices. Components whose source headers do not contain a separate NOTICE file do not gain an invented NOTICE.

The manifest intentionally exposes any primary-source discrepancy rather than asserting that all upstream licensing has a single unambiguous SPDX label.
