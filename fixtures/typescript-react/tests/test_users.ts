import { NotificationService } from "../app/notifications/service";
import { User } from "../app/users/models";
import { UserService } from "../app/users/service";

export function test_register_sends_notification(): string {
  const user = new User("user-1");
  const notifications = new NotificationService();
  const service = new UserService();
  return service.register(notifications, user);
}
