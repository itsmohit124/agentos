# Good architecture

This fixture is a small hexagonal catalog. Ground truth is `expected/architecture.json`
and expects no findings.

| Check | Why it should stay quiet |
|---|---|
| circular_dependency | Imports point inward only: adapter and controller depend on application or domain. Domain does not import them back. |
| fat_controller | `CatalogController` has two methods. The default threshold is 5. |
| domain_infrastructure | `sqlite3` is imported from `app/catalog/adapter/memory.py`, not from a domain file. |
| duplicate_terminology | The glossary has Product and Sku. Those names are not a synonym pair. |
| missing_tests | `tests/test_catalog.py` calls Product, Sku, ProductAggregate, and ProductService. |
| shallow_module | Every production module has either two or more symbols, or one class with two methods. |
| boundary_leakage | The domain module does not import an adapter, infrastructure, or controller module. |
| unwanted_direction | Application imports the domain and the port. It does not import the adapter. |
| oversized_surface | The largest module has three classes. The default threshold is 6. |

`ProductRepository` is a port. Missing-test detection covers entities, value objects, aggregates,
and domain services. A repository without its own test is not a finding.
