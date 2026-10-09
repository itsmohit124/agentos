import { NotificationService } from "../app/notifications/service.js";
import { User } from "../app/users/models.js";
import { UserService } from "../app/users/service.js";

export function test_register_sends_notification() {
  const user = new User("user-1");
  const notifications = new NotificationService();
  const service = new UserService();
  return service.register(notifications, user);
}
