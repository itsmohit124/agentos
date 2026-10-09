require_relative "../app/notifications/service"
require_relative "../app/users/models"
require_relative "../app/users/service"

class UserTests
  def test_register_sends_notification
    user = User.new("user-1")
    notifications = NotificationService.new
    service = UserService.new
    service.register(notifications, user)
  end
end
