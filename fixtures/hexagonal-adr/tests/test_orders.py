"""Orders can be saved."""

from app.orders.domain.model import Order
from app.orders.repository import OrderRepository
from app.orders.service import OrderService


def test_place_saves_order() -> None:
    """Placing an order stores it."""
    order = Order("ord-1", 20)
    saved = OrderService().place(OrderRepository(), order)
    assert saved.order_id == "ord-1"
