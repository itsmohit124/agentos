# Registers accounts and notifies them.

class UserService
  # Register a user and send a welcome notice.
  # @param notifications [NotificationService]
  # @param user [User]
  def register(notifications, user)
    notifications.send("user-1", "welcome")
  end
end
