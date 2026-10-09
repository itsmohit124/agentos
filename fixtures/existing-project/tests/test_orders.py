"""Tests for orders."""


def test_place_returns_id() -> None:
    """Placing an order returns its id."""
    service = OrderService()
    assert service.place("ord-1") == "ord-1"
