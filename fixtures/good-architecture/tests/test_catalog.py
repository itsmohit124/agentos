"""Tests for catalog domain types."""

from app.catalog.application.service import ProductService
from app.catalog.domain.model import Product, ProductAggregate, Sku


def test_product_price() -> None:
    product = Product("p-1", 10)
    sku = Sku("SKU-1")
    aggregate = ProductAggregate(product)
    service = ProductService()
    assert product.price() == 10
    assert sku.text() == "SKU-1"
    assert aggregate.root() == product
    assert service.describe(product) == "p-1"
