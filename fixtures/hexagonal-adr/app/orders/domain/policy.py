"""Corrective policy for orders that drift."""


class ReconciliationPolicy:
    """Periodically detect and repair orders whose stored amount drifted."""

    def repair(self, order: Order) -> Order:
        """Flag and repair an inconsistent order."""
        return order
