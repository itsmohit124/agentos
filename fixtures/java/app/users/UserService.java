package app.users;

/** Registers accounts and notifies them. */
public class UserService {
    public String register(NotificationService notifications, User user) {
        return notifications.send(user.userId, "welcome");
    }
}
