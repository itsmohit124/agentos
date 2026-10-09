# Ruby Rails fixture

A small existing service used as knowledge-engine ground truth. It is plain Ruby in the
shape of a Rails service (controller, service, repository). Nothing here is executed.

Call edges use YARD `@param name [Type]`, a type written above the method. `Type.new` is a
bare call of that constant. `require_relative` resolves against the file. A Ruby module is
stored as a class because it is a named method container and there is no namespace node kind;
this fixture does not declare one. Inheritance with `<` links only inside the same file.
Logical ids drop only a leading `src/` or `lib/`.
