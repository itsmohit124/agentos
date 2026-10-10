"""Catalog domain model. No infrastructure imports."""


class Product:
    """An entity with an identity."""

    def __init__(self, product_id: str, amount: int) -> None:
        if amount <= 0:
            raise ValueError("amount > 0")
        self.product_id = product_id
        self.amount = amount

    def label(self) -> str:
        return self.product_id

    def price(self) -> int:
        return self.amount


class Sku:
    """A value object for a stock keeping code."""

    def __init__(self, code: str) -> None:
        self.code = code

    def text(self) -> str:
        return self.code

    def same(self, code: str) -> bool:
        return self.code == code


class ProductAggregate:
    """Aggregate root for Product."""

    def __init__(self, product: Product) -> None:
        self.product = product

    def root(self) -> Product:
        return self.product

    def retitle(self, product: Product) -> Product:
        return product
