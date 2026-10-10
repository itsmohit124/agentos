# Bad architecture

Each file below is one seeded smell. Ground truth is `expected/architecture.json`.
A finding is expected only for the detector named here.

| Detector | File | Why it is a violation |
|---|---|---|
| circular_dependency | `app/cycle/left.py` and `app/cycle/right.py` | The two modules import each other. |
| fat_controller | `app/orders/controller.py` | `OrderController` has 7 methods. The default threshold is 5. |
| domain_infrastructure | `app/orders/domain/model.py` | The domain module imports `sqlalchemy`. |
| duplicate_terminology | `docs/spec/requirements.md` | Glossary terms Invoice and Bill are synonyms and are not merged. |
| missing_tests | `app/billing/domain/model.py` | `UntestedCharge` is a domain entity and no test calls it. |
| shallow_module | `app/billing/passthrough.py` | One class, one method. |
| boundary_leakage | `app/orders/domain/model.py` | The domain module imports `app.orders.adapter.db`. |
| unwanted_direction | `app/orders/application/checkout.py` | The application module imports the adapter. |
| oversized_surface | `app/billing/surface.py` | Seven classes. The default public-surface threshold is 6. |

`tests/test_orders.py` calls `Order` so that entity is not a missing-test finding.
`Order` is the only tested domain symbol in this fixture.
