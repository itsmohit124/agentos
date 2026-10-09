"""Tests for billing."""


def test_charge_returns_amount() -> None:
    """Charging returns the amount."""
    service = BillingService()
    assert service.charge(10) == 10
