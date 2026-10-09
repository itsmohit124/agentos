# Node Express fixture

A small existing service used as knowledge-engine ground truth. It is plain JavaScript in the
shape of an Express application (controller, service, repository). Nothing here is executed.

JavaScript has no parameter annotations. Call edges use JSDoc `@param {Type} name`, which is
a type written in the source. `new Type()` is a bare call of that type. Imports are ESM and
resolve relative to the file, dropping only a leading `src/` or `lib/`.
