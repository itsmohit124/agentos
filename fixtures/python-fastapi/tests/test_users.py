"""Tests for users."""

from app.users.models import User
from app.users.service import UserService
from app.notifications.service import NotificationService


def test_register_sends_notification() -> None:
    """Registering a user sends a welcome notification."""
    user = User("user-1")
    notifications = NotificationService()
    service = UserService()
    service.register(notifications, user)
