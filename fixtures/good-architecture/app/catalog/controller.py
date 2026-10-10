"""Thin HTTP controller."""

from app.catalog.application.service import ProductService


class CatalogController:
    """Handles catalog requests. Two actions only."""

    def show(self, service: ProductService) -> str:
        return "show"

    def update(self, service: ProductService) -> str:
        return "update"
