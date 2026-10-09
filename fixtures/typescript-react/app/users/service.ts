/** Registers accounts and notifies them. */

export class UserService {
  register(notifications: NotificationService, user: User): string {
    return notifications.send(user.userId, "welcome");
  }
}
