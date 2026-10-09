# TypeScript React fixture

A small existing service used as knowledge-engine ground truth. It is TypeScript in the
shape of a React application (view, controller, service, repository). Nothing here is executed.

Parameter annotations are the declared types used for call edges. A capitalized JSX tag is a
bare call of that component. An arrow assigned to a const is a function. Interface `extends`
and class `implements` link only to types in the same file. Imports resolve relative to the
file, dropping only a leading `src/` or `lib/`.
