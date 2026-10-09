"""User application service."""


class UserService:
    """Registers users and notifies them."""

    def register(self, notifications: NotificationService, user: User) -> str:
        """Register a user and send a welcome notice."""
        return notifications.send(user.user_id, "welcome")
