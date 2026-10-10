"""Application service. Depends inward on the domain and the port."""

from app.catalog.domain.model import Product
from app.catalog.ports.repository import ProductRepository


class ProductService:
    """Domain service for catalog changes."""

    def rename(self, repository: ProductRepository, product: Product) -> Product:
        """Save a product through the port."""
        return repository.save(product.product_id)

    def describe(self, product: Product) -> str:
        """Describe a product."""
        return product.label()
