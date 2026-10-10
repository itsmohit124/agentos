"""In-memory adapter. Depends inward. Infrastructure stays here."""

import sqlite3

from app.catalog.domain.model import Product


class MemoryCatalogStore:
    """Stores products outside the domain."""

    def save(self, product: Product) -> Product:
        return product

    def ping(self) -> str:
        _ = sqlite3
        return "ok"
