"""Fat controller. Seven actions, above the default threshold of 5."""


class OrderController:
    """Handles order HTTP actions."""

    def list_orders(self) -> str:
        return "list"

    def get_order(self) -> str:
        return "get"

    def create_order(self) -> str:
        return "create"

    def update_order(self) -> str:
        return "update"

    def delete_order(self) -> str:
        return "delete"

    def archive_order(self) -> str:
        return "archive"

    def export_orders(self) -> str:
        return "export"
