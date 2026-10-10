"""Port for catalog persistence."""


class ProductRepository:
    """Port. Adapters implement this."""

    def save(self, product_id: str) -> str:
        return product_id

    def get(self, product_id: str) -> str:
        return product_id
