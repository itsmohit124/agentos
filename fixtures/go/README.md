# Go fixture

A small existing service used as knowledge-engine ground truth. It is plain Go in the
shape of an HTTP service (controller, service, repository). Nothing here is executed.

Structs are classes. A struct field with a type and no name is embedding (`EXTENDS`, same
file only). Go does not declare `implements`, so an interface value is not an IMPLEMENTS
edge. Import paths use slashes that match the file module (`app/payments/service`). A
composite literal `Type{}` is a bare call of that type. Logical ids drop only a leading
`src/` or `lib/`.
