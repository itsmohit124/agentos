# Java fixture

A small existing service used as knowledge-engine ground truth. It is plain Java in the
shape of an HTTP service (controller, service, repository). Nothing here is executed.

Files are named after the public type, so the file stem stays in the logical id
(`software` plus the dotted path plus `class` plus the type name). `src/main/java` is not
used: only a leading `src/` or `lib/` would be stripped, and the rest of a Maven path would
stay in the id. `extends` and `implements` link only to types in the same file. `new Type()`
is a bare call of that type. Test methods are named `test_` so the Phase 2 method rule marks
them as tests. Java has no module-level assignment, so a status constant inside a class is
not a symbol node.
