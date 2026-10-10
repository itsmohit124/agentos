"""Tests Order so that entity is not reported as missing tests."""

from app.orders.domain.model import Order


def test_order_total() -> None:
    order = Order("o-1", 10)
    assert order.total() == 10
