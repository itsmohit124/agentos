/** Registers accounts and notifies them. */

export class UserService {
  /**
   * Register a user and send a welcome notice.
   * @param {NotificationService} notifications
   * @param {User} user
   * @returns {string}
   */
  register(notifications, user) {
    return notifications.send(user.userId, "welcome");
  }
}
